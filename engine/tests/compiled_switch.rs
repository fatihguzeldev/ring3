#[path = "support/compiled_switch.rs"]
#[allow(dead_code)]
mod pe;

use std::collections::BTreeSet;

use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        memory_helper::{HELPER_SIZE, encode_helper_result},
        x86::decode_state,
    },
    cpu::{
        dbt::{BlockSpec, CompileLimits},
        x86::{
            Register32,
            decode::{DecodedInstruction, decode_one},
            ir::{BranchTarget, Condition, EffectiveAddress, Location32, Operation, Value32},
        },
    },
    loader::{LinkedImageMetadata32V5, load_pe32_linked_v5_at},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::EngineInstance,
};

const KEY: u64 = 0xa534_0123_4567_89ab;

fn bytes(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    memory.read(GuestAddress(address), &mut output).unwrap();
    output
}

fn dword(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap())
}

fn loaded(base: u32) -> (EngineInstance, LinkedImageMetadata32V5) {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    let image = pe::fixture().image;
    engine.begin_image_input(image.len() as u32).unwrap();
    engine.append_image_input(0, &image).unwrap();
    let metadata = engine
        .load_pe32_linked_v5_input_at(base, pe::GATE_BASE)
        .unwrap();
    (engine, metadata)
}

fn blocks(
    memory: &AddressSpace,
    base: u32,
    rva: u32,
    entries: &[u32],
) -> Vec<Vec<DecodedInstruction>> {
    entries
        .iter()
        .map(|offset| {
            let mut pc = GuestAddress(base + rva + offset);
            let boundary = entries
                .iter()
                .filter(|other| *other > offset)
                .min()
                .map(|next| base + rva + next);
            let mut block = Vec::new();
            for _ in 0..CompileLimits::default().instructions {
                let instruction = decode_one(memory, pc).unwrap();
                pc = instruction.next_pc();
                let terminal = matches!(
                    instruction.operation(),
                    Operation::Jump { .. }
                        | Operation::ConditionalJump { .. }
                        | Operation::Call { .. }
                        | Operation::Return { .. }
                );
                assert!(boundary.is_none_or(|end| pc.0 <= end));
                block.push(instruction);
                if terminal || Some(pc.0) == boundary {
                    return block;
                }
            }
            panic!("fixture block exceeds unchanged instruction limit");
        })
        .collect()
}

fn specs(blocks: &[Vec<DecodedInstruction>]) -> Vec<BlockSpec> {
    blocks
        .iter()
        .map(|block| BlockSpec {
            entry: block[0].pc(),
            byte_length: block.last().unwrap().next_pc().0 - block[0].pc().0,
        })
        .collect()
}

fn request(engine: &mut EngineInstance, specs: &[BlockSpec], gates: &[(u32, u32)], entries: bool) {
    let stride = if entries { 4 } else { 8 };
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    for (index, spec) in specs.iter().enumerate() {
        let at = index * stride;
        transfer[at..at + 4].copy_from_slice(&spec.entry.0.to_le_bytes());
        if !entries {
            transfer[at + 4..at + 8].copy_from_slice(&spec.byte_length.to_le_bytes());
        }
    }
    for (index, (pc, id)) in gates.iter().enumerate() {
        let at = specs.len() * stride + index * 8;
        transfer[at..at + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[at + 4..at + 8].copy_from_slice(&id.to_le_bytes());
    }
}

#[test]
fn original_coff_addends_link_only_the_five_declared_dwords() {
    let object = include_bytes!("fixtures/p2-compiled-switch/kernel.obj");
    assert_eq!(object.len(), 715);
    assert_eq!(pe::KERNEL_RAW_TEXT, &object[220..279]);
    assert_eq!(pe::KERNEL_RAW_TABLE, &object[289..305]);
    assert_eq!(dword(pe::KERNEL_RAW_TEXT, 23), 0);
    let mut text = pe::KERNEL_RAW_TEXT.to_vec();
    text[23..27].copy_from_slice(&(pe::PREFERRED_BASE + pe::TABLE_RVA).to_le_bytes());
    assert_eq!(pe::KERNEL, text);
    let mut table = pe::KERNEL_RAW_TABLE.to_vec();
    for (index, target) in pe::TABLE_TARGETS.into_iter().enumerate() {
        assert_eq!(dword(pe::KERNEL_RAW_TABLE, index * 4), target);
        table[index * 4..index * 4 + 4]
            .copy_from_slice(&(pe::PREFERRED_BASE + pe::KERNEL_RVA + target).to_le_bytes());
    }
    assert_eq!(pe::TABLE, table);
    for (at, offset, symbol) in [
        (279, 23, 6),
        (305, 0, 0),
        (315, 4, 0),
        (325, 8, 0),
        (335, 12, 0),
    ] {
        assert_eq!(dword(object, at), offset);
        assert_eq!(dword(object, at + 4), symbol);
        assert_eq!(&object[at + 8..at + 10], &[6, 0]);
    }
    let fixture = pe::fixture();
    assert_eq!(fixture.main.len(), 182);
    assert_eq!(fixture.call_offsets, [29, 54, 79, 104, 129, 154]);
    assert_eq!(fixture.return_offsets, [34, 59, 84, 109, 134, 159]);
    assert_eq!(fixture.main_entries, [0, 34, 59, 84, 109, 134, 159]);
    assert_eq!(fixture.authored_fixups.len(), 14);
    assert_eq!(
        fixture
            .compiler_fixups
            .iter()
            .map(|fixup| fixup.rva)
            .collect::<Vec<_>>(),
        [0x2017, 0x4000, 0x4004, 0x4008, 0x400c]
    );
    let mut state = pe::INITIAL_STATE;
    let mut salt = pe::INITIAL_SALT;
    for (index, selector) in pe::SELECTORS.into_iter().enumerate() {
        state = match selector {
            0 => state.wrapping_add(7),
            1 => state ^ 0xa5a5_5a5a,
            2 => state.rotate_left(5),
            3 => state.wrapping_mul(3).wrapping_add(1),
            _ => state.wrapping_sub(selector),
        };
        salt ^= state;
        assert_eq!(state, pe::EXPECTED_STATES[index]);
        assert_eq!(salt, pe::EXPECTED_RETURNS[index]);
    }
}

#[test]
fn preferred_and_relocated_images_patch_exact_highlow_fields_and_keep_table_read_only() {
    let fixture = pe::fixture();
    assert_eq!(fixture.image.len(), 0xe00);
    for base in [pe::PREFERRED_BASE, pe::ACTUAL_BASE] {
        let (memory, metadata) = load_pe32_linked_v5_at(&fixture.image, base, pe::GATE_BASE, 8)
            .unwrap()
            .into_parts();
        assert_eq!(metadata.gate_count, 1);
        assert_eq!(
            metadata.gates[0].entry,
            GuestAddress(pe::GATE_BASE + pe::GATE_OFFSET)
        );
        let mut main = fixture.main.clone();
        let mut kernel = pe::KERNEL.to_vec();
        let mut table = pe::TABLE.to_vec();
        for fixup in fixture.fixups() {
            let (output, offset) = if fixup.rva < pe::KERNEL_RVA {
                (&mut main, fixup.rva - pe::MAIN_RVA)
            } else if fixup.rva < pe::DATA_RVA {
                (&mut kernel, fixup.rva - pe::KERNEL_RVA)
            } else {
                (&mut table, fixup.rva - pe::TABLE_RVA)
            };
            output[offset as usize..offset as usize + 4]
                .copy_from_slice(&(base + fixup.target_rva).to_le_bytes());
        }
        assert_eq!(bytes(&memory, base + pe::MAIN_RVA, main.len()), main);
        assert_eq!(bytes(&memory, base + pe::KERNEL_RVA, kernel.len()), kernel);
        assert_eq!(bytes(&memory, base + pe::TABLE_RVA, table.len()), table);
        assert_eq!(bytes(&memory, base + pe::MAIN_RVA + 182, 330), [0xcc; 330]);
        assert_eq!(bytes(&memory, base + pe::KERNEL_RVA + 59, 453), [0xcc; 453]);
        assert_eq!(bytes(&memory, base + pe::TABLE_RVA + 16, 496), [0; 496]);
        assert_eq!(
            dword(&bytes(&memory, base + pe::STATE_RVA, 4), 0),
            pe::INITIAL_STATE
        );
        assert_eq!(bytes(&memory, base + pe::OUTPUT_RVA, 48), [0; 48]);
        assert_eq!(
            dword(&bytes(&memory, base + pe::CANARY_RVA, 4), 0),
            0x1357_9bdf
        );
        assert_eq!(
            dword(&bytes(&memory, base + pe::IAT_EXIT_RVA, 4), 0),
            pe::GATE_BASE + pe::GATE_OFFSET
        );
        assert_eq!(bytes(&memory, base + 0x3ffc, 4), [0; 4]);
        for (rva, allowed) in [
            (0, [true, false, false]),
            (pe::MAIN_RVA, [true, false, true]),
            (pe::KERNEL_RVA, [true, false, true]),
            (pe::DATA_RVA, [true, true, false]),
            (pe::TABLE_RVA, [true, false, false]),
            (pe::FIXUP_RVA, [true, false, false]),
        ] {
            for (access, permitted) in [Access::Read, Access::Write, Access::Execute]
                .into_iter()
                .zip(allowed)
            {
                assert_eq!(
                    memory.resolve(GuestAddress(base + rva), access).is_ok(),
                    permitted
                );
            }
        }
        let mut actual_fixups = Vec::new();
        let mut offset = pe::FIXUP_RAW;
        for page in [pe::MAIN_RVA, pe::KERNEL_RVA, pe::TABLE_RVA] {
            assert_eq!(dword(&fixture.image, offset), page);
            let length = dword(&fixture.image, offset + 4) as usize;
            for entry in fixture.image[offset + 8..offset + length].chunks_exact(2) {
                let value = u16::from_le_bytes(entry.try_into().unwrap());
                if value != 0 {
                    assert_eq!(value >> 12, 3);
                    actual_fixups.push(page + u32::from(value & 4095));
                }
            }
            offset += length;
        }
        let mut expected = fixture.fixups().map(|fixup| fixup.rva).collect::<Vec<_>>();
        expected.sort_unstable();
        assert_eq!(actual_fixups, expected);
        assert_eq!(offset, pe::FIXUP_RAW + 64);
    }
}

#[test]
fn typed_compiler_dispatch_and_cdecl_caller_keep_the_original_cfg() {
    let fixture = pe::fixture();
    let (engine, _) = loaded(pe::ACTUAL_BASE);
    let memory = engine.memory().unwrap();
    let kernel = blocks(memory, pe::ACTUAL_BASE, pe::KERNEL_RVA, &pe::KERNEL_ENTRIES);
    assert_eq!(
        kernel.iter().map(Vec::len).collect::<Vec<_>>(),
        [7, 1, 2, 2, 3, 2, 1, 4]
    );
    assert_eq!(
        specs(&kernel)
            .iter()
            .map(|spec| spec.byte_length)
            .collect::<Vec<_>>(),
        [20, 7, 5, 5, 6, 8, 2, 6]
    );
    let starts = kernel
        .iter()
        .flatten()
        .map(|instruction| instruction.pc().0)
        .collect::<BTreeSet<_>>();
    assert_eq!(starts.len(), 22);
    for target in pe::TABLE_TARGETS {
        assert!(starts.contains(&(pe::ACTUAL_BASE + pe::KERNEL_RVA + target)));
    }
    let operation = |rva| {
        *decode_one(memory, GuestAddress(pe::ACTUAL_BASE + rva))
            .unwrap()
            .operation()
    };
    assert_eq!(
        operation(pe::KERNEL_RVA + 0x12),
        Operation::ConditionalJump {
            condition: Condition::Above,
            target: GuestAddress(pe::ACTUAL_BASE + pe::KERNEL_RVA + 0x33)
        }
    );
    assert_eq!(
        operation(pe::KERNEL_RVA + 0x14),
        Operation::Jump {
            target: BranchTarget::Indirect(Location32::Memory(EffectiveAddress {
                base: None,
                index: Some(Register32::Esi),
                scale: 4,
                displacement: pe::ACTUAL_BASE + pe::TABLE_RVA
            }))
        }
    );
    assert_eq!(
        operation(pe::KERNEL_RVA + 0x35),
        Operation::Move {
            destination: Location32::Memory(EffectiveAddress {
                base: Some(Register32::Edx),
                index: None,
                scale: 1,
                displacement: 0
            }),
            source: Value32::Register(Register32::Ecx)
        }
    );
    assert_eq!(
        operation(pe::KERNEL_RVA + 0x3a),
        Operation::Return { stack_adjust: 0 }
    );
    let caller = blocks(memory, pe::ACTUAL_BASE, pe::MAIN_RVA, &fixture.main_entries);
    assert_eq!(
        caller.iter().map(Vec::len).collect::<Vec<_>>(),
        [9, 8, 8, 8, 8, 8, 6]
    );
    for (index, call) in fixture.call_offsets.iter().enumerate() {
        assert_eq!(
            operation(pe::MAIN_RVA + call),
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress(pe::ACTUAL_BASE + pe::KERNEL_RVA))
            }
        );
        assert_eq!(
            operation(pe::MAIN_RVA + call - 4),
            Operation::Push {
                source: Value32::Register(Register32::Eax)
            }
        );
        assert_eq!(
            operation(pe::MAIN_RVA + call - 3),
            Operation::Push {
                source: Value32::Immediate(pe::SELECTORS[index])
            }
        );
        assert_eq!(
            operation(pe::MAIN_RVA + call - 1),
            Operation::Push {
                source: Value32::Register(Register32::Edi)
            }
        );
    }
}

#[test]
fn four_bound_profiles_admit_separate_default_sized_caller_and_compiler_units() {
    let fixture = pe::fixture();
    for profile in 0..4 {
        let (mut engine, metadata) = loaded(pe::ACTUAL_BASE);
        engine.start_loaded_image(pe::STACK_BASE, 1).unwrap();
        let state = decode_state(&engine.arena()[..56]).unwrap();
        assert_eq!(
            state.registers,
            [0, 0, 0, 0, pe::STACK_BASE + 4096, 0, 0, 0]
        );
        assert_eq!(state.eip, pe::ACTUAL_BASE + pe::MAIN_RVA);
        engine.arena_mut().unwrap()[X87_OFFSET + 40..X87_OFFSET + 120].fill(0xa5);
        let resident = profile >= 2;
        let entries = profile % 2 != 0;
        let mut retained = Vec::new();
        for (slot, (rva, offsets)) in [
            (pe::MAIN_RVA, fixture.main_entries.as_slice()),
            (pe::KERNEL_RVA, pe::KERNEL_ENTRIES.as_slice()),
        ]
        .into_iter()
        .enumerate()
        {
            let decoded = blocks(engine.memory().unwrap(), pe::ACTUAL_BASE, rva, offsets);
            let mut specs = specs(&decoded);
            let gates = if slot == 0 {
                vec![(metadata.gates[0].entry.0, metadata.gates[0].id)]
            } else {
                vec![]
            };
            for (pc, _) in &gates {
                specs.push(BlockSpec {
                    entry: GuestAddress(*pc),
                    byte_length: 2,
                });
            }
            assert!(specs.len() <= CompileLimits::default().blocks);
            assert!(
                decoded.iter().map(Vec::len).sum::<usize>() + gates.len()
                    <= CompileLimits::default().instructions
            );
            request(&mut engine, &specs, &gates, entries);
            let before = engine.arena().to_vec();
            let wasm = if resident {
                let id = if entries {
                    engine.compile_resident_entries(specs.len() as u32, gates.len() as u32)
                } else {
                    engine.compile_resident_with_gates(specs.len() as u32, gates.len() as u32)
                }
                .unwrap()
                .get();
                let wasm = engine.resident_bytes(id).unwrap().to_vec();
                engine
                    .acknowledge_resident_installation(KEY, id, slot as u32)
                    .unwrap();
                retained.push((id, wasm.clone()));
                wasm
            } else {
                let generation = if entries {
                    engine.compile_entries(specs.len() as u32, gates.len() as u32)
                } else {
                    engine.compile_with_gates(specs.len() as u32, gates.len() as u32)
                }
                .unwrap();
                engine.guard(KEY, generation).unwrap();
                engine.artifact_bytes().unwrap().to_vec()
            };
            assert_eq!(engine.arena(), before);
            assert!(wasm.starts_with(b"\0asm\x01\0\0\0"));
            assert!(wasm.len() <= CompileLimits::default().wasm_bytes);
        }
        for (id, wasm) in retained {
            assert_eq!(engine.resident_bytes(id).unwrap(), wasm);
            engine.guard_resident(KEY, id).unwrap();
        }
    }
}

#[test]
fn table_read_permission_faults_and_repairs_do_not_invalidate_compiled_source() {
    for resident in [false, true] {
        let (mut engine, _) = loaded(pe::ACTUAL_BASE);
        engine.start_loaded_image(pe::STACK_BASE, 1).unwrap();
        let kernel = blocks(
            engine.memory().unwrap(),
            pe::ACTUAL_BASE,
            pe::KERNEL_RVA,
            &pe::KERNEL_ENTRIES,
        );
        request(&mut engine, &specs(&kernel), &[], false);
        let owner = if resident {
            engine.compile_resident(8).unwrap().get()
        } else {
            u64::from(engine.compile(8).unwrap())
        };
        let table = pe::ACTUAL_BASE + pe::TABLE_RVA;
        let old = bytes(engine.memory().unwrap(), table, 16);
        engine.protect(table, 1, 0).unwrap();
        let current = |engine: &EngineInstance| {
            if resident {
                engine.guard_resident(KEY, owner)
            } else {
                engine.guard(KEY, owner as u32)
            }
        };
        current(&engine).unwrap();
        for _ in 0..2 {
            let mut expected = engine.arena().to_vec();
            encode_helper_result(
                Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(table),
                    access: Access::Read,
                    reason: FaultReason::Permission,
                })),
                &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
            )
            .unwrap();
            engine.read32(table).unwrap();
            assert_eq!(engine.arena(), expected);
            current(&engine).unwrap();
        }
        let before = engine.arena().to_vec();
        engine.protect(table, 1, 1).unwrap();
        assert_eq!(engine.arena(), before);
        assert_eq!(bytes(engine.memory().unwrap(), table, 16), old);
        current(&engine).unwrap();
        let mut expected = engine.arena().to_vec();
        encode_helper_result(
            Ok(pe::ACTUAL_BASE + pe::KERNEL_RVA + pe::TABLE_TARGETS[0]),
            &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .unwrap();
        engine.read32(table).unwrap();
        assert_eq!(engine.arena(), expected);
    }
}
