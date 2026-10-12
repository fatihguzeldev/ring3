use super::support_pe32_mutable_records as pe;

use std::collections::BTreeSet;

use ring3_engine::{
    abi::{arena::TRANSFER_OFFSET, x86::decode_state},
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            Register32,
            decode::{DecodedInstruction, decode_one},
            ir::{
                BinaryKind, BranchTarget, ByteRegister, ByteValue, Condition, EffectiveAddress,
                ExtensionKind, Location32, Operation, SmallSource, SmallWidth, Value32,
                WordReadArithmeticKind, WordValue,
            },
        },
    },
    loader::{LinkedImageMetadata32V5, load_pe32_linked_v5_at},
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryError},
    process::EngineInstance,
    windows::WindowsApi32,
};

const KEY: u64 = 0xa529_1234_5678_abcd;

fn bytes(memory: &AddressSpace, address: u32, length: usize) -> Vec<u8> {
    let mut result = vec![0; length];
    memory.read(GuestAddress(address), &mut result).unwrap();
    result
}

fn word(memory: &AddressSpace, address: u32) -> u32 {
    u32::from_le_bytes(bytes(memory, address, 4).try_into().unwrap())
}

fn loaded(count: u8) -> (EngineInstance, LinkedImageMetadata32V5) {
    let fixture = pe::fixture(count);
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine
        .begin_image_input(fixture.image.len() as u32)
        .unwrap();
    engine.append_image_input(0, &fixture.image).unwrap();
    let receipt = engine
        .load_pe32_linked_v5_input_at(pe::ACTUAL_BASE, pe::GATE_BASE)
        .unwrap();
    (engine, receipt)
}

fn terminal(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Jump { .. }
            | Operation::ConditionalJump { .. }
            | Operation::CountBranch { .. }
            | Operation::Call { .. }
            | Operation::Return { .. }
    )
}

fn decoded_group(memory: &AddressSpace, group: &pe::EntryGroup) -> Vec<Vec<DecodedInstruction>> {
    let text = pe::ACTUAL_BASE + pe::TEXT_RVA;
    group
        .offsets
        .iter()
        .map(|offset| {
            let mut pc = GuestAddress(text + offset);
            let boundary = group
                .offsets
                .iter()
                .filter(|other| *other > offset)
                .min()
                .map(|other| text + other);
            let mut result = Vec::new();
            for _ in 0..CompileLimits::default().instructions {
                let instruction = decode_one(memory, pc).unwrap();
                pc = instruction.next_pc();
                let stop = terminal(instruction.operation()) || Some(pc.0) == boundary;
                assert!(boundary.is_none_or(|boundary| pc.0 <= boundary));
                result.push(instruction);
                if stop {
                    return result;
                }
            }
            panic!("group {} exceeds the instruction budget", group.name);
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
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    let stride = if entries { 4 } else { 8 };
    for (index, spec) in specs.iter().enumerate() {
        let start = index * stride;
        transfer[start..start + 4].copy_from_slice(&spec.entry.0.to_le_bytes());
        if !entries {
            transfer[start + 4..start + 8].copy_from_slice(&spec.byte_length.to_le_bytes());
        }
    }
    for (index, (pc, id)) in gates.iter().enumerate() {
        let start = specs.len() * stride + index * 8;
        transfer[start..start + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[start + 4..start + 8].copy_from_slice(&id.to_le_bytes());
    }
}

#[test]
fn finite_images_preserve_frozen_mutable_kernel_and_two_count_payloads() {
    let first = pe::fixture(0);
    assert_eq!(pe::KERNEL.len(), 101);
    assert_eq!(pe::record_bytes().len(), 128);
    assert_eq!(
        &pe::record_bytes()[..8],
        &[0x80, 0, 0, 0x80, 0xff, 0xff, 1, 0xa0]
    );
    assert_eq!(
        &pe::record_bytes()[120..],
        &[1, 12, 0xff, 0xff, 0xff, 0x7f, 0x10, 0xa0]
    );
    let receipt = include_str!("../../fixtures/p2-mutable-records/receipt.json");
    assert!(receipt.contains(pe::KERNEL_SHA256));
    assert!(receipt.contains("\"entry_offset\": 0"));
    assert!(receipt.contains("\"extent_end_offset\": 101"));
    for count in pe::COUNTS {
        let fixture = pe::fixture(count);
        assert_eq!(fixture.image.len(), 2560);
        assert_eq!(fixture.program.len(), 613);
        assert_eq!(&fixture.image[0x178..0x180], b".text\0\0\0");
        assert_eq!(
            u32::from_le_bytes(fixture.image[0x180..0x184].try_into().unwrap()),
            613
        );
        assert_eq!(&fixture.program[pe::KERNEL_OFFSET as usize..], pe::KERNEL);
        assert_eq!(
            &fixture.image[pe::TEXT_RAW..pe::TEXT_RAW + 613],
            fixture.program
        );
        assert_eq!(
            &fixture.image[pe::DATA_RAW + 0x20..pe::DATA_RAW + 0xa0],
            pe::record_bytes()
        );
        let mut expected = first.image.clone();
        expected[pe::TEXT_RAW + 53] = count;
        expected[pe::TEXT_RAW + 70] = count;
        assert_eq!(
            fixture.image, expected,
            "only the two authored count immediates change"
        );
        assert_eq!(fixture.fixups.len(), 8);
        assert!(
            fixture
                .fixups
                .iter()
                .all(|fixup| fixup.offset < pe::KERNEL_OFFSET)
        );
        assert_eq!(fixture.fixups.last().unwrap().offset, 113);
        let manifest = fixture.manifest_json();
        assert!(manifest.contains(&format!("\"count\":{count},")));
        let [first, second] = pe::expected_checksums(count);
        assert!(manifest.contains(&format!("\"expected_checksum1\":{first},")));
        assert!(manifest.contains(&format!("\"expected_checksum2\":{second},")));
        assert!(manifest.contains("\"leaf_calls\":[55,72]"));
        assert!(manifest.contains("\"leaf_returns\":[60,77]"));
        assert!(manifest.contains(pe::KERNEL_SHA256));
        assert!(manifest.contains(pe::RECORD_SHA256));
    }
    assert_eq!(pe::expected_checksums(0), [pe::SEED; 2]);
    assert_ne!(pe::expected_checksums(1), pe::expected_checksums(2));
}

#[test]
fn preferred_and_relocated_images_change_only_authored_fixups_and_named_iat() {
    let fixture = pe::fixture(16);
    for base in [pe::PREFERRED_BASE, pe::ACTUAL_BASE] {
        let (memory, receipt) = load_pe32_linked_v5_at(&fixture.image, base, pe::GATE_BASE, 8)
            .unwrap()
            .into_parts();
        assert_eq!(receipt.image.image_base, base);
        assert_eq!(receipt.image.image_size, pe::IMAGE_SIZE);
        assert_eq!(receipt.image.entry_point, base + pe::TEXT_RVA);
        assert_eq!(receipt.image.mapped_pages, 5);
        assert_eq!(receipt.gate_count, 3);
        let mut expected_text = fixture.image[pe::TEXT_RAW..pe::DATA_RAW].to_vec();
        for fixup in &fixture.fixups {
            let offset = fixup.offset as usize;
            assert_eq!(
                u32::from_le_bytes(expected_text[offset..offset + 4].try_into().unwrap()),
                pe::PREFERRED_BASE + fixup.target_rva
            );
            expected_text[offset..offset + 4]
                .copy_from_slice(&(base + fixup.target_rva).to_le_bytes());
        }
        assert_eq!(bytes(&memory, base + pe::TEXT_RVA, 1024), expected_text);
        assert_eq!(
            bytes(&memory, base + pe::TEXT_RVA + pe::KERNEL_OFFSET, 101),
            pe::KERNEL
        );
        assert_eq!(
            bytes(&memory, base + pe::TEXT_RVA + 1024, 3072),
            vec![0; 3072]
        );
        let mut expected_data = fixture.image[pe::DATA_RAW..pe::FIXUP_RAW].to_vec();
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
            let offset = (iat - pe::DATA_RVA) as usize;
            expected_data[offset..offset + 4].copy_from_slice(&gate.entry.0.to_le_bytes());
        }
        assert_eq!(bytes(&memory, base + pe::DATA_RVA, 512), expected_data);
        assert_eq!(
            bytes(&memory, base + pe::RECORD_RVA, 128),
            pe::record_bytes()
        );
        assert_eq!(word(&memory, base + pe::CHECKSUM1_RVA), 0);
        assert_eq!(word(&memory, base + pe::CHECKSUM2_RVA), 0);
        assert_eq!(word(&memory, base + pe::CANARY_RVA), 0);
        assert_eq!(
            bytes(&memory, base + pe::FIXUP_RVA, 512),
            fixture.image[pe::FIXUP_RAW..pe::FIXUP_RAW + 512]
        );
        for (address, permissions) in [
            (base, [true, false, false]),
            (base + pe::TEXT_RVA, [true, false, true]),
            (base + pe::DATA_RVA, [true, true, false]),
            (base + pe::FIXUP_RVA, [true, false, false]),
            (pe::GATE_BASE, [true, false, true]),
        ] {
            for (access, permitted) in [Access::Read, Access::Write, Access::Execute]
                .into_iter()
                .zip(permissions)
            {
                assert_eq!(
                    memory.resolve(GuestAddress(address), access).is_ok(),
                    permitted
                );
            }
        }
    }
}

#[test]
fn compiler_cfg_narrow_stores_and_live_second_call_fit_default_limits() {
    let fixture = pe::fixture(16);
    let (engine, _) = loaded(16);
    let memory = engine.memory().unwrap();
    let text = pe::ACTUAL_BASE + pe::TEXT_RVA;
    let mut pcs = BTreeSet::new();
    for (group, expected_counts) in fixture
        .groups
        .iter()
        .zip([vec![5, 10, 4, 6, 6, 2], vec![8, 1, 26, 5]])
    {
        let blocks = decoded_group(memory, group);
        assert_eq!(
            blocks.iter().map(Vec::len).collect::<Vec<_>>(),
            expected_counts
        );
        assert!(blocks.len() <= CompileLimits::default().blocks);
        assert!(
            blocks.iter().map(Vec::len).sum::<usize>() <= CompileLimits::default().instructions
        );
        for instruction in blocks.iter().flatten() {
            assert!(pcs.insert(instruction.pc().0));
        }
    }
    assert_eq!(pcs.len(), 73);
    let operation = |offset| {
        *decode_one(memory, GuestAddress(text + offset))
            .unwrap()
            .operation()
    };
    assert_eq!(
        operation(45),
        Operation::ConditionalJump {
            condition: Condition::NotEqual,
            target: GuestAddress(text + 34)
        }
    );
    assert_eq!(
        operation(47),
        Operation::Push {
            source: Value32::Immediate(pe::SEED)
        }
    );
    assert_eq!(
        operation(52),
        Operation::Push {
            source: Value32::Immediate(16)
        }
    );
    for call in pe::LEAF_CALLS {
        assert_eq!(
            operation(call),
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress(text + pe::KERNEL_OFFSET))
            }
        );
    }
    for offset in pe::LEAF_RETURNS {
        assert_eq!(
            operation(offset),
            Operation::Binary {
                kind: BinaryKind::Add,
                destination: Location32::Register(Register32::Esp),
                source: Value32::Immediate(12)
            }
        );
    }
    assert_eq!(
        operation(63),
        Operation::Move {
            destination: Location32::Memory(EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: pe::ACTUAL_BASE + pe::CHECKSUM1_RVA,
                ..Default::default()
            }),
            source: Value32::Register(Register32::Eax)
        }
    );
    assert_eq!(
        operation(68),
        Operation::Push {
            source: Value32::Register(Register32::Eax)
        }
    );
    assert_eq!(
        operation(69),
        Operation::Push {
            source: Value32::Immediate(16)
        }
    );
    assert_eq!(
        operation(71),
        Operation::Push {
            source: Value32::Register(Register32::Edi)
        }
    );
    assert_eq!(
        operation(80),
        Operation::Move {
            destination: Location32::Memory(EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: pe::ACTUAL_BASE + pe::CHECKSUM2_RVA,
                ..Default::default()
            }),
            source: Value32::Register(Register32::Eax)
        }
    );
    let address = |displacement| EffectiveAddress {
        base: Some(Register32::Ebp),
        index: Some(Register32::Esi),
        scale: 8,
        displacement,
        ..Default::default()
    };
    assert_eq!(
        operation(0x219),
        Operation::Extend {
            kind: ExtensionKind::Zero,
            destination: Register32::Ebx,
            source: SmallSource::Memory {
                address: address(1),
                width: SmallWidth::Byte
            }
        }
    );
    assert_eq!(
        operation(0x225),
        Operation::ReadArithmeticWord {
            kind: WordReadArithmeticKind::Add,
            destination: Register32::Edi,
            address: address(4)
        }
    );
    assert_eq!(
        operation(0x22a),
        Operation::StoreByte {
            address: address(1),
            source: ByteValue::Register(ByteRegister::Bl)
        }
    );
    assert_eq!(
        operation(pe::FAULT_STORE_OFFSET),
        Operation::StoreWord {
            address: address(4),
            source: WordValue::Register(Register32::Edi)
        }
    );
    for (offset, width, destination, displacement) in [
        (0x235, SmallWidth::Byte, Register32::Ecx, 0),
        (0x23a, SmallWidth::Word, Register32::Ebp, 2),
    ] {
        assert_eq!(
            operation(offset),
            Operation::Extend {
                kind: ExtensionKind::Sign,
                destination,
                source: SmallSource::Memory {
                    address: address(displacement),
                    width
                }
            }
        );
    }
    assert_eq!(
        operation(0x23f),
        Operation::SignedMultiply {
            destination: Register32::Ebp,
            source: Location32::Register(Register32::Ecx),
            immediate: None
        }
    );
    assert_eq!(
        operation(0x24e),
        Operation::ConditionalMove {
            condition: Condition::NotEqual,
            destination: Register32::Ebp,
            source: Register32::Ecx
        }
    );
    assert_eq!(
        operation(0x25e),
        Operation::ConditionalJump {
            condition: Condition::NotEqual,
            target: GuestAddress(text + 0x212)
        }
    );
    assert_eq!(operation(0x264), Operation::Return { stack_adjust: 0 });
    assert_eq!(pcs.range(text + 0x200..text + 0x265).count(), 40);
}

#[test]
fn four_bound_profiles_admit_three_units_before_any_provider_allocation() {
    for count in [0, 16] {
        for profile in 0..4 {
            let fixture = pe::fixture(count);
            let (mut engine, receipt) = loaded(count);
            engine.start_loaded_image(pe::STACK_BASE, 1).unwrap();
            let startup = decode_state(&engine.arena()[..56]).unwrap();
            assert_eq!(
                startup.registers,
                [0, 0, 0, 0, pe::STACK_BASE + 4096, 0, 0, 0]
            );
            assert_eq!(startup.eip, pe::ACTUAL_BASE + pe::TEXT_RVA);
            assert_eq!(startup.eflags, 2);
            assert!(
                matches!(engine.memory().unwrap().resolve(GuestAddress(0x1000_0000), Access::Read), Err(MemoryError::Fault(fault)) if fault.reason == FaultReason::Unmapped)
            );
            let resident = profile >= 2;
            let entries = profile % 2 == 1;
            let mut retained = Vec::new();
            for (slot, group) in fixture.groups.iter().enumerate() {
                let blocks = decoded_group(engine.memory().unwrap(), group);
                let specs = specs(&blocks);
                let entry_pcs = specs.iter().map(|spec| spec.entry).collect::<Vec<_>>();
                let error = Some(CompileError::Instruction {
                    pc: specs[0].entry,
                    cause: InstructionError::BackendUnsupported,
                });
                assert_eq!(
                    compile_region(engine.memory().unwrap(), &specs, CompileLimits::default())
                        .err(),
                    error
                );
                assert_eq!(
                    compile_entry_region(
                        engine.memory().unwrap(),
                        &entry_pcs,
                        CompileLimits::default()
                    )
                    .err(),
                    error
                );
                request(&mut engine, &specs, &[], entries);
                let before = engine.arena().to_vec();
                if resident {
                    let id = if entries {
                        engine.compile_resident_entries(specs.len() as u32, 0)
                    } else {
                        engine.compile_resident(specs.len() as u32)
                    }
                    .unwrap()
                    .get();
                    assert_eq!(engine.arena(), before);
                    let bytes = engine.resident_bytes(id).unwrap();
                    assert!(bytes.starts_with(b"\0asm\x01\0\0\0"));
                    assert!(bytes.len() <= CompileLimits::default().wasm_bytes);
                    retained.push((id, bytes.to_vec()));
                    engine
                        .acknowledge_resident_installation(KEY, id, slot as u32)
                        .unwrap();
                    for instruction in blocks.iter().flatten() {
                        assert_eq!(
                            engine
                                .lookup_installed_resident(KEY, instruction.pc().0)
                                .unwrap()
                                .unit_id,
                            id
                        );
                    }
                } else {
                    let generation = if entries {
                        engine.compile_entries(specs.len() as u32, 0)
                    } else {
                        engine.compile(specs.len() as u32)
                    }
                    .unwrap();
                    assert_eq!(engine.arena(), before);
                    assert!(
                        engine.artifact_bytes().unwrap().len()
                            <= CompileLimits::default().wasm_bytes
                    );
                    engine.guard(KEY, generation).unwrap();
                }
            }
            let gates = receipt.gates[..receipt.gate_count as usize]
                .iter()
                .map(|gate| (gate.entry.0, gate.id))
                .collect::<Vec<_>>();
            let gate_specs = gates
                .iter()
                .map(|(pc, _)| BlockSpec {
                    entry: GuestAddress(*pc),
                    byte_length: 2,
                })
                .collect::<Vec<_>>();
            request(&mut engine, &gate_specs, &gates, entries);
            let before = engine.arena().to_vec();
            if resident {
                let id = if entries {
                    engine.compile_resident_entries(3, 3)
                } else {
                    engine.compile_resident_with_gates(3, 3)
                }
                .unwrap()
                .get();
                assert_eq!(engine.arena(), before);
                engine
                    .acknowledge_resident_installation(KEY, id, 2)
                    .unwrap();
                for (pc, _) in gates {
                    assert_eq!(
                        engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
                        id
                    );
                }
                for (id, bytes) in retained {
                    assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
                    engine.guard_resident(KEY, id).unwrap();
                }
            } else {
                let generation = if entries {
                    engine.compile_entries(3, 3)
                } else {
                    engine.compile_with_gates(3, 3)
                }
                .unwrap();
                assert_eq!(engine.arena(), before);
                engine.guard(KEY, generation).unwrap();
            }
            assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), startup);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 6);
            assert_eq!(
                bytes(
                    engine.memory().unwrap(),
                    pe::ACTUAL_BASE + pe::RECORD_RVA,
                    128
                ),
                pe::record_bytes()
            );
        }
    }
}

fn transform(records: &mut [u8; pe::RECORD_BYTES], count: u8, mut acc: u32) -> u32 {
    for record in records.chunks_exact_mut(8).take(usize::from(count)) {
        acc = acc.rotate_left(5);
        let tag = record[1] ^ acc as u8;
        let weight = u16::from_le_bytes([record[4], record[5]]).wrapping_add((acc >> 16) as u16);
        record[1] = tag;
        record[4..6].copy_from_slice(&weight.to_le_bytes());
        let product =
            i32::from(record[0] as i8) * i32::from(i16::from_le_bytes([record[2], record[3]]));
        if tag & 1 == 1 {
            acc ^= product as u32;
        } else {
            acc = acc.wrapping_add(product as u32);
        }
        acc = acc.wrapping_add(u32::from(weight) * u32::from(tag));
    }
    acc
}

#[test]
fn independent_record_arithmetic_pins_both_live_passes_and_untouched_fields() {
    let original = pe::record_bytes();
    for count in pe::COUNTS {
        let mut actual = original;
        let expected = pe::expected_checksums(count);
        let first = transform(&mut actual, count, pe::SEED);
        assert_eq!(first, expected[0]);
        assert_eq!(actual, pe::expected_records(count, 0));
        let before_second = actual;
        let second = transform(&mut actual, count, first);
        assert_eq!(second, expected[1]);
        assert_eq!(actual, pe::expected_records(count, 1));
        for (index, (initial, result)) in original
            .chunks_exact(8)
            .zip(actual.chunks_exact(8))
            .enumerate()
        {
            for offset in [0, 2, 3, 6, 7] {
                assert_eq!(result[offset], initial[offset]);
            }
            if index >= usize::from(count) {
                assert_eq!(result, initial);
            }
        }
        if count == 0 {
            assert_eq!(actual, original);
        } else {
            assert_ne!(before_second, original);
            assert_ne!(actual, before_second);
            let mut wrong_seed = before_second;
            transform(&mut wrong_seed, count, pe::SEED);
            assert_ne!(
                wrong_seed, actual,
                "second pass must use returned first checksum"
            );
        }
        assert_eq!(
            pe::expected_retired(count),
            219 + 2 * pe::leaf_retired(count)
        );
    }
    assert_eq!(
        &pe::expected_records(1, 0)[..8],
        &[0x80, 0xfb, 0, 0x80, 0xb6, 0xd5, 1, 0xa0]
    );
    assert_eq!(
        &pe::expected_records(16, 1)[24..32],
        &[1, 0x9e, 0, 0x80, 0x7f, 0xff, 4, 0xa0]
    );
    assert_eq!(pe::expected_checksums(16), [0x8cf0_b199, 0x4438_b326]);
}
