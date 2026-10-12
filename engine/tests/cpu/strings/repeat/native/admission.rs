use ring3_engine::{
    cpu::x86::{
        decode::decode_one,
        ir::{AddressSize, Operation, RepeatCondition, StringKind, StringWidth},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> Operation {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    *decode_one(&memory, GuestAddress(0x1000))
        .unwrap()
        .operation()
}

#[test]
fn repeated_string_family_decodes_all_operations_widths_and_repeat_conditions() {
    for (opcode, kind) in [
        (0xa4, StringKind::Move),
        (0xaa, StringKind::Store),
        (0xac, StringKind::Load),
        (0xa6, StringKind::Compare),
        (0xae, StringKind::Scan),
    ] {
        for width in [StringWidth::Byte, StringWidth::Word, StringWidth::Dword] {
            for prefix in [0xf2, 0xf3] {
                let mut bytes = vec![prefix];
                if width == StringWidth::Word {
                    bytes.push(0x66);
                }
                bytes.push(opcode + u8::from(width != StringWidth::Byte));
                let repeat = if matches!(kind, StringKind::Compare | StringKind::Scan) {
                    if prefix == 0xf2 {
                        RepeatCondition::NotEqual
                    } else {
                        RepeatCondition::Equal
                    }
                } else {
                    RepeatCondition::Always
                };
                assert_eq!(
                    decode(&bytes),
                    Operation::RepeatString {
                        kind,
                        width,
                        repeat,
                        address_size: AddressSize::Bits32,
                        source_segment: None,
                    },
                    "{bytes:02x?}"
                );
            }
        }
    }
}

#[test]
fn repeated_string_address16_and_source_override_are_explicit_in_ir() {
    use ring3_engine::cpu::x86::SegmentRegister;
    assert_eq!(
        decode(&[0x64, 0x67, 0xf3, 0xa5]),
        Operation::RepeatString {
            kind: StringKind::Move,
            width: StringWidth::Dword,
            repeat: RepeatCondition::Always,
            address_size: AddressSize::Bits16,
            source_segment: Some(SegmentRegister::Fs),
        }
    );
}

#[test]
fn plain_strings_retain_address_size_and_segment_without_repeat_count_semantics() {
    use ring3_engine::cpu::x86::SegmentRegister;
    for (opcode, kind) in [
        (0xa4, StringKind::Move),
        (0xaa, StringKind::Store),
        (0xac, StringKind::Load),
        (0xa6, StringKind::Compare),
        (0xae, StringKind::Scan),
    ] {
        for width in [StringWidth::Byte, StringWidth::Word, StringWidth::Dword] {
            let mut bytes = vec![0x65, 0x67];
            if width == StringWidth::Word {
                bytes.push(0x66);
            }
            bytes.push(opcode + u8::from(width != StringWidth::Byte));
            assert_eq!(
                decode(&bytes),
                Operation::String {
                    kind,
                    width,
                    address_size: AddressSize::Bits16,
                    source_segment: Some(SegmentRegister::Gs)
                }
            );
        }
    }
}

#[test]
fn every_repeated_string_width_compiles_in_both_bound_owners() {
    for opcode in [0xa4, 0xaa, 0xac, 0xa6, 0xae] {
        for width in [1, 2, 4] {
            for resident in [false, true] {
                let mut bytes = vec![0xf3];
                if width == 2 {
                    bytes.push(0x66);
                }
                bytes.push(opcode + u8::from(width != 1));
                bytes.extend_from_slice(&[0xeb, 0xfe]);
                let mut engine = EngineInstance::new(1, 0x5250).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(&bytes);
                engine.upload(0x1000, bytes.len() as u32).unwrap();
                engine.protect(0x1000, 1, 4).unwrap();
                let input = engine.arena_mut().unwrap();
                input[140..144].copy_from_slice(&0x1000u32.to_le_bytes());
                input[144..148].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                if resident {
                    engine.compile_resident(1).unwrap();
                } else {
                    engine.compile(1).unwrap();
                }
            }
        }
    }
}
