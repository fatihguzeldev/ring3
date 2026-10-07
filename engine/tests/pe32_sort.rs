#[path = "support/pe32_sort.rs"]
mod pe;

use std::collections::BTreeSet;

use ring3_engine::{
    abi::{arena::TRANSFER_OFFSET, x86::decode_state},
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, DecodedInstruction, decode_one},
            ir::{
                BinaryKind, BranchTarget, Condition, EffectiveAddress, Location32, Operation,
                RotateKind, ShiftCount, Value32,
            },
        },
    },
    loader::{LinkedImageMetadata32V5, load_pe32_linked_v5_at},
    memory::{Access, AddressSpace, GuestAddress, MemoryError},
    process::{EngineInstance, HostError},
    windows::WindowsApi32,
};

const KEY: u64 = 0xa435_1234_5678_abcd;

fn bytes(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    bytes
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(bytes(memory, address, 4).try_into().unwrap())
}

fn loaded() -> (EngineInstance, LinkedImageMetadata32V5) {
    let image = pe::image();
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine.begin_image_input(image.len() as u32).unwrap();
    engine.append_image_input(0, &image).unwrap();
    let receipt = engine
        .load_pe32_linked_v5_input_at(pe::ACTUAL_BASE, pe::GATE_BASE)
        .unwrap();
    (engine, receipt)
}

fn request(engine: &mut EngineInstance, entries: &[u32], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    for (index, entry) in entries.iter().enumerate() {
        transfer[index * 4..index * 4 + 4].copy_from_slice(&entry.to_le_bytes());
    }
    for (index, (pc, id)) in gates.iter().enumerate() {
        let offset = entries.len() * 4 + index * 8;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
    }
}

fn decoded_group(memory: &AddressSpace, group: &pe::EntryGroup) -> Vec<Vec<DecodedInstruction>> {
    group
        .offsets
        .iter()
        .map(|offset| {
            let mut pc = GuestAddress(pe::ACTUAL_BASE + pe::TEXT_RVA + offset);
            let mut instructions = Vec::new();
            for _ in 0..CompileLimits::default().instructions {
                let decoded = decode_one(memory, pc).unwrap();
                let terminal = matches!(
                    decoded.operation(),
                    Operation::Jump { .. }
                        | Operation::ConditionalJump { .. }
                        | Operation::Call { .. }
                        | Operation::Return { .. }
                );
                pc = decoded.next_pc();
                instructions.push(decoded);
                if terminal {
                    return instructions;
                }
            }
            panic!("{} entry {offset:#x} has no bounded terminator", group.name);
        })
        .collect()
}

fn indirect_call(rva: u32) -> Operation {
    Operation::Call {
        target: BranchTarget::Indirect(Location32::Memory(EffectiveAddress {
            base: None,
            index: None,
            scale: 1,
            displacement: pe::ACTUAL_BASE + rva,
        })),
    }
}

#[test]
fn authored_image_relocates_only_declared_words_and_binds_named_imports() {
    let fixture = pe::fixture();
    assert_eq!(fixture.image, pe::image());
    assert_eq!(fixture.image.len(), 0xa00);
    assert_eq!(fixture.program.len(), 533);
    assert_eq!(fixture.fixups.len(), 8);
    assert_eq!(pe::WORDS, 256);
    assert_eq!(pe::BATCHES, 10);
    assert_eq!(pe::SEED, 0x6d2b_79f5);
    assert_eq!(
        fixture
            .groups
            .iter()
            .map(|group| group.name)
            .collect::<Vec<_>>(),
        ["main", "fill", "sort", "checksum"]
    );
    let manifest = fixture.manifest_json();
    assert!(manifest.contains("\"program_hex\":\""));
    assert!(manifest.contains("\"batches\":10"));
    assert!(manifest.contains("\"name\":\"VirtualAlloc\""));
    assert!(manifest.contains("\"name\":\"VirtualFree\""));
    assert!(manifest.contains("\"name\":\"ExitProcess\""));

    for base in [pe::PREFERRED_BASE, pe::ACTUAL_BASE] {
        let (memory, receipt) = load_pe32_linked_v5_at(&fixture.image, base, pe::GATE_BASE, 8)
            .unwrap()
            .into_parts();
        assert_eq!(receipt.image.image_base, base);
        assert_eq!(receipt.image.image_size, pe::IMAGE_SIZE);
        assert_eq!(receipt.image.entry_point, base + pe::TEXT_RVA);
        assert_eq!(receipt.image.mapped_pages, 5);
        assert_eq!(receipt.gate_count, 3);
        assert_eq!(memory.mapped_pages(), 5);
        let mut expected = fixture.image[pe::TEXT_RAW..pe::DATA_RAW].to_vec();
        for fixup in &fixture.fixups {
            let offset = fixup.offset as usize;
            assert_eq!(
                u32::from_le_bytes(expected[offset..offset + 4].try_into().unwrap()),
                pe::PREFERRED_BASE + fixup.target_rva
            );
            expected[offset..offset + 4].copy_from_slice(&(base + fixup.target_rva).to_le_bytes());
        }
        assert_eq!(bytes(&memory, base + pe::TEXT_RVA, 1024), expected);
        assert_eq!(
            bytes(&memory, base + pe::TEXT_RVA + 1024, 3072),
            vec![0; 3072]
        );
        assert_eq!(
            bytes(&memory, base + pe::FIXUP_RVA, 512),
            fixture.image[pe::FIXUP_RAW..pe::FIXUP_RAW + 512]
        );
        assert_eq!(word(&memory, base + pe::CHECKSUM_RVA), 0);
        assert_eq!(word(&memory, base + pe::AGGREGATE_RVA), 0);
        assert_eq!(word(&memory, base + pe::COUNTER_RVA), 10);
        assert_eq!(word(&memory, base + pe::CANARY_RVA), 0);
        for (index, (api, iat)) in [
            (WindowsApi32::ExitProcess, pe::IAT_EXIT_RVA),
            (WindowsApi32::VirtualAlloc, pe::IAT_ALLOC_RVA),
            (WindowsApi32::VirtualFree, pe::IAT_FREE_RVA),
        ]
        .into_iter()
        .enumerate()
        {
            let gate = receipt.gates[index];
            assert_eq!(gate.id, api.id());
            assert_eq!(gate.entry.0, pe::GATE_BASE + pe::GATE_OFFSETS[index]);
            assert_eq!(word(&memory, base + iat), gate.entry.0);
            assert_eq!(bytes(&memory, gate.entry.0, 2), [0x0f, 0x0b]);
        }
        for (address, allowed) in [
            (base + pe::TEXT_RVA, [true, false, true]),
            (base + pe::DATA_RVA, [true, true, false]),
            (base + pe::FIXUP_RVA, [true, false, false]),
            (pe::GATE_BASE, [true, false, true]),
        ] {
            for (access, allowed) in [Access::Read, Access::Write, Access::Execute]
                .into_iter()
                .zip(allowed)
            {
                assert_eq!(
                    memory.resolve(GuestAddress(address), access).is_ok(),
                    allowed
                );
            }
        }
    }
}

#[test]
fn all_authored_entries_decode_disjoint_typed_blocks_with_unsigned_sort_branches() {
    let fixture = pe::fixture();
    let (engine, _) = loaded();
    let memory = engine.memory().unwrap();
    let text = pe::ACTUAL_BASE + pe::TEXT_RVA;
    let expected = [
        vec![
            (0, 5, indirect_call(pe::IAT_ALLOC_RVA)),
            (
                20,
                2,
                Operation::Call {
                    target: BranchTarget::Direct(GuestAddress(text + 0x100)),
                },
            ),
            (
                27,
                1,
                Operation::Call {
                    target: BranchTarget::Direct(GuestAddress(text + 0x180)),
                },
            ),
            (
                32,
                1,
                Operation::Call {
                    target: BranchTarget::Direct(GuestAddress(text + 0x200)),
                },
            ),
            (37, 6, indirect_call(pe::IAT_FREE_RVA)),
            (
                62,
                2,
                Operation::ConditionalJump {
                    condition: Condition::NotEqual,
                    target: GuestAddress(text),
                },
            ),
            (74, 2, indirect_call(pe::IAT_EXIT_RVA)),
        ],
        vec![
            (
                0x100,
                16,
                Operation::ConditionalJump {
                    condition: Condition::NotEqual,
                    target: GuestAddress(text + 0x10c),
                },
            ),
            (0x129, 1, Operation::Return { stack_adjust: 0 }),
        ],
        vec![
            (
                0x180,
                5,
                Operation::ConditionalJump {
                    condition: Condition::BelowOrEqual,
                    target: GuestAddress(text + 0x19c),
                },
            ),
            (
                0x18e,
                3,
                Operation::ConditionalJump {
                    condition: Condition::BelowOrEqual,
                    target: GuestAddress(text + 0x19c),
                },
            ),
            (
                0x195,
                3,
                Operation::Jump {
                    target: BranchTarget::Direct(GuestAddress(text + 0x18a)),
                },
            ),
            (
                0x19c,
                4,
                Operation::ConditionalJump {
                    condition: Condition::Below,
                    target: GuestAddress(text + 0x185),
                },
            ),
            (0x1a7, 1, Operation::Return { stack_adjust: 0 }),
        ],
        vec![
            (
                0x200,
                8,
                Operation::ConditionalJump {
                    condition: Condition::NotEqual,
                    target: GuestAddress(text + 0x209),
                },
            ),
            (0x214, 1, Operation::Return { stack_adjust: 0 }),
        ],
    ];
    let mut all_pcs = BTreeSet::new();
    let mut count = 0;
    for (group, rows) in fixture.groups.iter().zip(expected) {
        let blocks = decoded_group(memory, group);
        assert_eq!(blocks.len(), rows.len());
        let mut ranges = Vec::new();
        for (block, (offset, instructions, terminal)) in blocks.iter().zip(rows) {
            assert_eq!(block[0].pc(), GuestAddress(text + offset));
            assert_eq!(block.len(), instructions);
            assert_eq!(*block.last().unwrap().operation(), terminal);
            ranges.push((block[0].pc().0, block.last().unwrap().next_pc().0));
            for instruction in block {
                assert!(all_pcs.insert(instruction.pc().0), "duplicate decoded pc");
                count += 1;
            }
        }
        for (index, (start, end)) in ranges.iter().enumerate() {
            for (other_start, other_end) in &ranges[..index] {
                assert!(
                    *end <= *other_start || *other_end <= *start,
                    "overlapping block ranges"
                );
            }
        }
    }
    assert_eq!(count, 61);
    for offset in [0x10c, 0x185, 0x18a, 0x19c, 0x209] {
        assert!(
            all_pcs.contains(&(text + offset)),
            "backward target is an admitted interior pc"
        );
    }
    assert_eq!(
        *decode_one(memory, GuestAddress(text + 0x185))
            .unwrap()
            .operation(),
        Operation::Lea {
            destination: Register32::Esi,
            address: EffectiveAddress {
                base: Some(Register32::Edi),
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0,
            }
        }
    );
    assert_eq!(
        *decode_one(memory, GuestAddress(text + 0x191))
            .unwrap()
            .operation(),
        Operation::Binary {
            kind: BinaryKind::Cmp,
            destination: Location32::Register(Register32::Ebx),
            source: Value32::Register(Register32::Eax)
        }
    );
    assert_eq!(
        *decode_one(memory, GuestAddress(text + 0x209))
            .unwrap()
            .operation(),
        Operation::Rotate {
            kind: RotateKind::Left,
            destination: Register32::Eax,
            count: ShiftCount::Immediate(5)
        }
    );
}

#[test]
fn grouped_bound_compilation_admits_five_units_without_accessing_unmapped_array() {
    let fixture = pe::fixture();
    let (mut engine, receipt) = loaded();
    engine.start_loaded_image(pe::STACK_BASE, 1).unwrap();
    let startup = decode_state(&engine.arena()[..56]).unwrap();
    assert_eq!(startup.eip, pe::ACTUAL_BASE + pe::TEXT_RVA);
    assert_eq!(
        startup.registers,
        [0, 0, 0, 0, pe::STACK_BASE + 4096, 0, 0, 0]
    );
    assert_eq!(startup.eflags, 2);
    let mut ids = Vec::new();
    let mut decoded_pcs = Vec::new();
    for (ordinal, group) in fixture.groups.iter().enumerate() {
        let rows = decoded_group(engine.memory().unwrap(), group);
        let specs = rows
            .iter()
            .map(|row| BlockSpec {
                entry: row[0].pc(),
                byte_length: row.last().unwrap().next_pc().0 - row[0].pc().0,
            })
            .collect::<Vec<_>>();
        let entries = specs.iter().map(|spec| spec.entry).collect::<Vec<_>>();
        let unsupported_offset = [0, 0x121, 0x188, 0x20c][ordinal];
        let error = Some(CompileError::Instruction {
            pc: GuestAddress(pe::ACTUAL_BASE + pe::TEXT_RVA + unsupported_offset),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            compile_region(engine.memory().unwrap(), &specs, CompileLimits::default()).err(),
            error
        );
        assert_eq!(
            compile_entry_region(engine.memory().unwrap(), &entries, CompileLimits::default())
                .err(),
            error
        );
        let raw_entries = entries.iter().map(|pc| pc.0).collect::<Vec<_>>();
        request(&mut engine, &raw_entries, &[]);
        let arena = engine.arena().to_vec();
        let id = engine
            .compile_resident_entries(entries.len() as u32, 0)
            .unwrap()
            .get();
        assert_eq!(engine.arena(), arena);
        let module = engine.resident_bytes(id).unwrap();
        assert!(module.starts_with(b"\0asm\x01\0\0\0"));
        assert!(module.len() <= CompileLimits::default().wasm_bytes);
        engine
            .acknowledge_resident_installation(KEY, id, ordinal as u32)
            .unwrap();
        for row in rows {
            for instruction in row {
                assert_eq!(
                    engine
                        .lookup_installed_resident(KEY, instruction.pc().0)
                        .unwrap()
                        .unit_id,
                    id
                );
                decoded_pcs.push(instruction.pc().0);
            }
        }
        ids.push(id);
    }
    let gate_specs = receipt.gates[..receipt.gate_count as usize]
        .iter()
        .map(|gate| (gate.entry.0, gate.id))
        .collect::<Vec<_>>();
    request(
        &mut engine,
        &gate_specs.iter().map(|(pc, _)| *pc).collect::<Vec<_>>(),
        &gate_specs,
    );
    let arena = engine.arena().to_vec();
    let gate_id = engine.compile_resident_entries(3, 3).unwrap().get();
    assert_eq!(engine.arena(), arena);
    engine
        .acknowledge_resident_installation(KEY, gate_id, 4)
        .unwrap();
    ids.push(gate_id);
    assert_eq!(ids.len(), 5);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 5);
    assert_eq!(decoded_pcs.len(), 61);
    assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), startup);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 6);
    let fault = engine
        .memory()
        .unwrap()
        .resolve(GuestAddress(0x1000_0000), Access::Read)
        .unwrap_err();
    assert!(
        matches!(fault, MemoryError::Fault(fault) if fault.reason == ring3_engine::memory::FaultReason::Unmapped)
    );
    for id in &ids {
        engine.guard_resident(KEY, *id).unwrap();
    }

    request(&mut engine, &[pe::ACTUAL_BASE + pe::TEXT_RVA + 86], &[]);
    let before = engine.arena().to_vec();
    let retained = ids
        .iter()
        .map(|id| engine.resident_bytes(*id).unwrap().to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        engine.compile_resident_entries(1, 0),
        Err(HostError::Resident(RegistryError::Compile(
            CompileError::Instruction {
                pc: GuestAddress(pe::ACTUAL_BASE + pe::TEXT_RVA + 96),
                cause: InstructionError::Decode(DecodeError::Unsupported(
                    UnsupportedFeature::Opcode
                ))
            }
        )))
    );
    assert_eq!(engine.arena(), before);
    for (id, bytes) in ids.iter().zip(retained) {
        assert_eq!(engine.resident_bytes(*id).unwrap(), bytes);
        engine.guard_resident(KEY, *id).unwrap();
    }
    assert_eq!(
        engine.start_loaded_image(pe::STACK_BASE, 1),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
}
