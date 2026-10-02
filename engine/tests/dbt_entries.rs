use ring3_engine::cpu::UnsupportedFeature;
use ring3_engine::cpu::dbt::{
    ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
    compile_region, prepare_entry_region,
};
use ring3_engine::cpu::x86::decode::DecodeError;
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryFault, PageRange, Permissions,
};
use ring3_engine::process::{EngineInstance, HostError};

fn range(address: u32, pages: u32) -> PageRange {
    PageRange::new(GuestAddress(address), pages).unwrap()
}

fn code(pc: u32, bytes: &[u8]) -> AddressSpace {
    let first = pc & !0xfff;
    let pages = (u64::from(pc - first) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut memory = AddressSpace::new(pages + 2).unwrap();
    memory
        .map_zeroed(range(first, pages), Permissions::ALL)
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}

fn spec(entry: u32, byte_length: u32) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(entry),
        byte_length,
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

#[test]
fn standalone_seed_discovers_authored_block_and_matches_explicit_emission() {
    // mov eax,0x12345678; add eax,1; jmp next; unsupported successor is outside the block.
    let bytes = [0xb8, 0x78, 0x56, 0x34, 0x12, 0x83, 0xc0, 1, 0xeb, 0, 0x40];
    let memory = code(0x1000, &bytes);
    let snapshot = memory
        .snapshot_code(GuestAddress(0x1000), bytes.len())
        .unwrap();
    let prepared =
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (1, 3)
    );
    assert!(prepared.is_current(&memory));
    let discovered =
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    let explicit = compile_region(&memory, &[spec(0x1000, 10)], CompileLimits::default()).unwrap();
    assert_eq!(discovered.metadata(), explicit.metadata());
    assert_eq!(
        discovered.wasm_bytes(&memory).unwrap(),
        explicit.wasm_bytes(&memory).unwrap()
    );
    assert!(memory.is_code_current(&snapshot));
    let mut unchanged = [0; 11];
    memory.read(GuestAddress(0x1000), &mut unchanged).unwrap();
    assert_eq!(unchanged, bytes);
}

#[test]
fn input_order_and_supplied_boundaries_match_independent_explicit_spans() {
    let mut bytes = vec![0x40; 24];
    bytes[..8].copy_from_slice(&[0xb8, 1, 2, 3, 4, 0x90, 0xeb, 0]);
    bytes[16..23].copy_from_slice(&[0xb9, 5, 6, 7, 8, 0xeb, 0xfe]);
    let memory = code(0x1000, &bytes);
    let entries = [
        GuestAddress(0x1010),
        GuestAddress(0x1000),
        GuestAddress(0x1005),
    ];
    let prepared = prepare_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (3, 5)
    );
    let discovered = compile_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
    let ordered = compile_region(
        &memory,
        &[spec(0x1010, 7), spec(0x1000, 5), spec(0x1005, 3)],
        CompileLimits::default(),
    )
    .unwrap();
    let sorted = compile_region(
        &memory,
        &[spec(0x1000, 5), spec(0x1005, 3), spec(0x1010, 7)],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(
        discovered.wasm_bytes(&memory).unwrap(),
        ordered.wasm_bytes(&memory).unwrap()
    );
    assert_ne!(
        discovered.wasm_bytes(&memory).unwrap(),
        sorted.wasm_bytes(&memory).unwrap()
    );
}

#[test]
fn control_terminators_do_not_fetch_fallthrough_or_follow_missing_targets() {
    for bytes in [
        &[0xeb, 0x7f][..],
        &[0xe9, 0xff, 0xff, 0xff, 0x7f][..],
        &[0x74, 0x7f][..],
        &[0x0f, 0x8d, 0xff, 0xff, 0xff, 0x7f][..],
    ] {
        let pc = 0x2000 - bytes.len() as u32;
        let mut memory = code(pc, bytes);
        memory
            .protect(range(0x1000, 1), Permissions::EXECUTE)
            .unwrap();
        let prepared =
            prepare_entry_region(&memory, &[GuestAddress(pc)], CompileLimits::default()).unwrap();
        let compiled =
            compile_entry_region(&memory, &[GuestAddress(pc)], CompileLimits::default()).unwrap();
        assert_eq!(prepared.instruction_count(), 1);
        memory
            .map_zeroed(range(0x2000, 1), Permissions::ALL)
            .unwrap();
        memory.write(GuestAddress(0x2000), &[0x40]).unwrap();
        memory.protect(range(0x2000, 1), Permissions::READ).unwrap();
        assert!(prepared.is_current(&memory));
        compiled.wasm_bytes(&memory).unwrap();
    }
}

#[test]
fn limits_and_structural_seeds_are_checked_before_guest_fetch() {
    let memory = AddressSpace::new(1).unwrap();
    for limits in [
        CompileLimits {
            blocks: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            blocks: 9,
            ..CompileLimits::default()
        },
        CompileLimits {
            instructions: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            instructions: 65,
            ..CompileLimits::default()
        },
        CompileLimits {
            wasm_bytes: 0,
            ..CompileLimits::default()
        },
        CompileLimits {
            wasm_bytes: 65537,
            ..CompileLimits::default()
        },
    ] {
        assert_eq!(
            prepare_entry_region(&memory, &[], limits).err(),
            Some(CompileError::InvalidLimits)
        );
    }
    for entries in [
        vec![],
        vec![GuestAddress(0x1000), GuestAddress(0x1000)],
        (0..9).map(GuestAddress).collect(),
    ] {
        assert_eq!(
            prepare_entry_region(&memory, &entries, CompileLimits::default()).err(),
            Some(CompileError::InvalidBlocks)
        );
    }
    assert_eq!(
        prepare_entry_region(
            &memory,
            &[GuestAddress(1), GuestAddress(2)],
            CompileLimits {
                blocks: 1,
                ..CompileLimits::default()
            }
        )
        .err(),
        Some(CompileError::InvalidBlocks)
    );
    let memory = code(0, &[0x90, 0xeb, 0]);
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0)], CompileLimits::default())
            .unwrap()
            .instruction_count(),
        2
    );
}

#[test]
fn seeds_inside_consumed_instructions_are_rejected_in_both_orders() {
    let memory = code(0x1000, &[0xb8, 0x90, 0x90, 0x90, 0x90, 0xeb, 0]);
    for entries in [
        [GuestAddress(0x1000), GuestAddress(0x1002)],
        [GuestAddress(0x1002), GuestAddress(0x1000)],
    ] {
        assert_eq!(
            prepare_entry_region(&memory, &entries, CompileLimits::default()).err(),
            Some(CompileError::InvalidBlocks)
        );
    }
}

#[test]
fn standalone_admission_rejects_decodable_out_of_profile_before_termination() {
    for bytes in [
        &[0x8b, 0x03][..],
        &[0x89, 0x03][..],
        &[0xc7, 0x03, 1, 0, 0, 0][..],
        &[0x50][..],
        &[0x58][..],
        &[0xff, 0x33][..],
        &[0x8f, 0x03][..],
        &[0xff, 0xe0][..],
        &[0xff, 0x23][..],
        &[0xff, 0xd0][..],
        &[0xff, 0x13][..],
        &[0xe8, 0, 0, 0, 0][..],
        &[0xc3][..],
        &[0xc2, 8, 0][..],
        &[0x03, 0x03][..],
        &[0x31, 0xc0][..],
        &[0x8d, 0x03][..],
        &[0x0f, 0xb6, 0xc0][..],
    ] {
        let mut instruction = vec![0x90];
        instruction.extend_from_slice(bytes);
        let memory = code(0x1000, &instruction);
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            Some(instruction_error(
                0x1001,
                InstructionError::BackendUnsupported
            )),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn decode_faults_retain_exact_pc_execute_address_reason_and_attempted_length() {
    let memory = AddressSpace::new(1).unwrap();
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1234)], CompileLimits::default()).err(),
        Some(instruction_error(
            0x1234,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1234),
                fault: MemoryFault {
                    address: GuestAddress(0x1234),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: 1,
            })
        ))
    );
    let mut memory = code(0x1fff, &[0xb8]);
    for permission in [None, Some(Permissions::READ)] {
        if let Some(permission) = permission {
            memory.map_zeroed(range(0x2000, 1), permission).unwrap();
        }
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default()).err(),
            Some(instruction_error(
                0x1fff,
                InstructionError::Decode(DecodeError::MemoryFault {
                    pc: GuestAddress(0x1fff),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: if permission.is_some() {
                            FaultReason::Permission
                        } else {
                            FaultReason::Unmapped
                        }
                    },
                    length: 2,
                })
            ))
        );
    }
    memory.protect(range(0x1000, 1), Permissions::READ).unwrap();
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1fff)], CompileLimits::default()).err(),
        Some(instruction_error(
            0x1fff,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1fff),
                fault: MemoryFault {
                    address: GuestAddress(0x1fff),
                    access: Access::Execute,
                    reason: FaultReason::Permission
                },
                length: 1,
            })
        ))
    );
}

#[test]
fn invalid_and_unsupported_decoder_errors_remain_distinct_from_backend_rejection() {
    for (bytes, cause) in [
        (&[0x8d, 0xc0][..], DecodeError::InvalidEncoding),
        (
            &[0x0f, 0x0b][..],
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            &[0xd9, 0xe8][..],
            DecodeError::Unsupported(UnsupportedFeature::FloatingPoint),
        ),
    ] {
        let memory = code(0x1000, bytes);
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            Some(instruction_error(0x1000, InstructionError::Decode(cause)))
        );
    }
}

#[test]
fn global_and_lower_instruction_caps_fail_before_the_next_needed_decode() {
    let mut bytes = vec![0x90; 63];
    bytes.extend_from_slice(&[0xeb, 0, 0x40]);
    let memory = code(0x1000, &bytes);
    let entries: Vec<_> = (0..8)
        .map(|index| GuestAddress(0x1000 + index * 8))
        .collect();
    let prepared = prepare_entry_region(&memory, &entries, CompileLimits::default()).unwrap();
    assert_eq!(
        (prepared.block_count(), prepared.instruction_count()),
        (8, 64)
    );
    let memory = code(0x1fc0, &[0x90; 64]);
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1fc0)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0x90, 0x40]);
    assert_eq!(
        prepare_entry_region(
            &memory,
            &[GuestAddress(0x1000)],
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            }
        )
        .err(),
        Some(CompileError::InstructionLimit)
    );
    let memory = code(0x1000, &[0x90, 0xeb, 0]);
    let limits = CompileLimits {
        blocks: 1,
        instructions: 2,
        wasm_bytes: 65536,
    };
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(0x1000)], limits)
            .unwrap()
            .instruction_count(),
        2
    );
    assert_eq!(
        compile_entry_region(
            &memory,
            &[GuestAddress(0x1000)],
            CompileLimits {
                wasm_bytes: 1,
                ..limits
            }
        )
        .err(),
        Some(CompileError::WasmLimit)
    );
}

#[test]
fn exact_address_space_end_stops_without_wrapping_fetch_to_zero() {
    for (pc, bytes) in [
        (u32::MAX, &[0x90][..]),
        (u32::MAX - 4, &[0xb8, 1, 2, 3, 4][..]),
        (u32::MAX - 1, &[0xeb, 0][..]),
    ] {
        let memory = code(pc, bytes);
        let limits = CompileLimits {
            instructions: 1,
            ..CompileLimits::default()
        };
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(pc)], limits)
                .unwrap()
                .instruction_count(),
            1
        );
        let discovered = compile_entry_region(&memory, &[GuestAddress(pc)], limits).unwrap();
        let explicit = compile_region(&memory, &[spec(pc, bytes.len() as u32)], limits).unwrap();
        assert_eq!(
            discovered.wasm_bytes(&memory).unwrap(),
            explicit.wasm_bytes(&memory).unwrap()
        );
    }
    let memory = code(u32::MAX, &[0xb8]);
    assert_eq!(
        prepare_entry_region(&memory, &[GuestAddress(u32::MAX)], CompileLimits::default()).err(),
        Some(instruction_error(
            u32::MAX,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(u32::MAX),
                fault: MemoryFault {
                    address: GuestAddress(u32::MAX),
                    access: Access::Execute,
                    reason: FaultReason::AddressOverflow
                },
                length: 2,
            })
        ))
    );
}

#[test]
fn discovered_cross_page_snapshots_track_consumed_pages_and_address_space_identity() {
    let bytes = [0xb8, 1, 2, 3, 4, 0xeb, 0];
    for changed_page in [0x1000, 0x2000] {
        let mut memory = code(0x1ffe, &bytes);
        let prepared =
            prepare_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default())
                .unwrap();
        let compiled =
            compile_entry_region(&memory, &[GuestAddress(0x1ffe)], CompileLimits::default())
                .unwrap();
        let other = code(0x1ffe, &bytes);
        assert!(!prepared.is_current(&other));
        assert_eq!(
            compiled.wasm_bytes(&other),
            Err(ArtifactError::CodeInvalidated)
        );
        memory
            .map_zeroed(range(0x3000, 1), Permissions::ALL)
            .unwrap();
        memory.write(GuestAddress(0x3000), &[0x90]).unwrap();
        assert!(prepared.is_current(&memory));
        memory
            .protect(range(changed_page, 1), Permissions::ALL)
            .unwrap();
        assert!(!prepared.is_current(&memory));
        assert_eq!(
            compiled.wasm_bytes(&memory),
            Err(ArtifactError::CodeInvalidated)
        );
    }
}

fn embedded(pc: u32, bytes: &[u8]) -> EngineInstance {
    let first = pc & !0xfff;
    let pages = (u64::from(pc - first) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, 0x1234_5678_abcd_ef01).unwrap();
    engine.map(first, pages, 7).unwrap();
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
    engine
}

fn entry_descriptors(engine: &mut EngineInstance, entries: &[u32], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    for (index, entry) in entries.iter().enumerate() {
        transfer[index * 4..index * 4 + 4].copy_from_slice(&entry.to_le_bytes());
    }
    for (index, (entry, id)) in gates.iter().enumerate() {
        let offset = entries.len() * 4 + index * 8;
        transfer[offset..offset + 4].copy_from_slice(&entry.to_le_bytes());
        transfer[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
    }
}

#[test]
fn embedded_discovery_admits_current_memory_stack_and_indirect_profile_without_data_reads() {
    for bytes in [
        &[0x8b, 0x03, 0x89, 0x0b, 0xc7, 0x03, 1, 2, 3, 4, 0xeb, 0][..],
        &[
            0x50, 0x68, 1, 2, 3, 4, 0x6a, 0xff, 0x58, 0xff, 0x33, 0x8f, 0x03, 0xc2, 8, 0,
        ][..],
        &[0xff, 0xd0][..],
        &[0xff, 0x13][..],
        &[0xff, 0xe0][..],
        &[0xff, 0x23][..],
        &[0xe8, 0xff, 0xff, 0xff, 0x7f][..],
        &[0xc3][..],
    ] {
        let pc = 0x2000 - bytes.len() as u32;
        let mut engine = embedded(pc, bytes);
        engine.protect(0x1000, 1, 4).unwrap();
        entry_descriptors(&mut engine, &[pc], &[]);
        let arena = engine.arena().to_vec();
        assert_eq!(engine.compile_entries(1, 0), Ok(1), "{bytes:02x?}");
        assert_eq!(engine.arena(), arena);
        engine.artifact_bytes().unwrap();
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    }
    for bytes in [&[0x03, 0x03][..], &[0x31, 0xc0][..], &[0x8d, 0x03][..]] {
        let mut engine = embedded(0x1000, bytes);
        entry_descriptors(&mut engine, &[0x1000], &[]);
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.compile_entries(1, 0),
            Err(HostError::Compile(instruction_error(
                0x1000,
                InstructionError::BackendUnsupported
            )))
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.generation(), 0);
    }
}

#[test]
fn registered_gate_slots_share_the_global_cap_in_both_seed_orders() {
    for ordinary_slots in [63, 64] {
        let mut bytes = vec![0x90; ordinary_slots];
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        let gate_pc = 0x1000 + ordinary_slots as u32;
        for entries in [[0x1000, gate_pc], [gate_pc, 0x1000]] {
            let mut engine = embedded(0x1000, &bytes);
            entry_descriptors(&mut engine, &entries, &[(gate_pc, 77)]);
            let arena = engine.arena().to_vec();
            let expected = if ordinary_slots == 63 {
                Ok(1)
            } else {
                Err(HostError::Compile(CompileError::InstructionLimit))
            };
            assert_eq!(engine.compile_entries(2, 1), expected);
            assert_eq!(engine.arena(), arena);
        }
    }
}

#[test]
fn gate_structure_and_interior_overlaps_are_rejected_before_unrelated_decode() {
    for gates in [
        vec![(0x1002, 0)],
        vec![(0x1003, 17)],
        vec![(0x1002, 17), (0x1002, 18)],
        vec![(0x1000, 17), (0x1002, 17)],
    ] {
        let mut engine = embedded(0x1000, &[0x40, 0x90, 0x0f, 0x0b]);
        entry_descriptors(&mut engine, &[0x1000, 0x1002], &gates);
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.compile_entries(2, gates.len() as u32),
            Err(HostError::Compile(CompileError::InvalidGates))
        );
        assert_eq!(engine.arena(), arena);
    }
    for entries in [[0x1000, 0x1001], [0x1001, 0x1000]] {
        let mut engine = embedded(0x1000, &[0xb8, 0x0f, 0x0b, 0, 0, 0xeb, 0]);
        entry_descriptors(&mut engine, &entries, &[(0x1001, 17)]);
        assert_eq!(
            engine.compile_entries(2, 1),
            Err(HostError::Compile(CompileError::InvalidBlocks))
        );
    }
    let mut engine = embedded(u32::MAX - 1, &[0x0f, 0x0b]);
    entry_descriptors(&mut engine, &[u32::MAX], &[(u32::MAX, 17)]);
    assert_eq!(
        engine.compile_entries(1, 1),
        Err(HostError::Compile(CompileError::InvalidBlocks))
    );
}

#[test]
fn gates_fetch_exactly_two_execute_bytes_and_accept_the_final_valid_span() {
    let mut engine = embedded(0x1fff, &[0x0f]);
    entry_descriptors(&mut engine, &[0x1fff], &[(0x1fff, 17)]);
    assert_eq!(
        engine.compile_entries(1, 1),
        Err(HostError::Compile(instruction_error(
            0x1fff,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x1fff),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: 2,
            })
        )))
    );
    let mut engine = embedded(u32::MAX - 1, &[0x0f, 0x0b]);
    entry_descriptors(&mut engine, &[u32::MAX - 1], &[(u32::MAX - 1, 0xffff_ffff)]);
    assert_eq!(engine.compile_entries(1, 1), Ok(1));
    engine.artifact_bytes().unwrap();
}
