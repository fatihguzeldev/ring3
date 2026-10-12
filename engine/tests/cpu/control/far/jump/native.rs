use ring3_engine::{
    cpu::x86::decode::{DecodeError, decode_one},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};

fn decode(bytes: &[u8]) -> Result<(), DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let decoded = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert!(memory.is_code_current(decoded.code_snapshot()));
    Ok(())
}

#[test]
fn immediate_far_jump_decodes_both_operand_widths() {
    for bytes in [
        &[0xea, 0x78, 0x56, 0x34, 0x12, 0x43, 0][..],
        &[0x66, 0xea, 0x78, 0x56, 0x43, 0],
    ] {
        assert_eq!(decode(bytes), Ok(()));
    }
}

#[test]
fn memory_far_jump_decodes_both_widths_segments_and_address_sizes() {
    for word in [false, true] {
        for (prefix, operand) in [
            (&[][..], &[0xff, 0x2f][..]),
            (&[0x64][..], &[0xff, 0x2f]),
            (&[0x67][..], &[0xff, 0x2e, 0, 0x50]),
            (&[][..], &[0xff, 0x6d, 0]),
        ] {
            let mut bytes = if word { vec![0x66] } else { vec![] };
            bytes.extend(prefix);
            bytes.extend(operand);
            assert_eq!(decode(&bytes), Ok(()));
        }
    }
}

#[test]
fn far_jump_lock_register_and_reserved_repeat_encodings_remain_refused() {
    for bytes in [
        &[0xff, 0xe8][..],
        &[0xf0, 0xff, 0x2f],
        &[0xf0, 0xea, 0, 0x20, 0, 0, 0x43, 0],
    ] {
        assert_eq!(decode(bytes), Err(DecodeError::InvalidEncoding));
    }
    for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
        let mut bytes = prefix.to_vec();
        bytes.extend([0xff, 0x2f]);
        assert!(matches!(decode(&bytes), Err(DecodeError::Unsupported(_))));
    }
}

#[test]
fn far_jump_capture_terminates_every_bound_profile_without_reading_target_memory() {
    for bytes in [
        &[0xea, 0, 0x20, 0, 0, 0x43, 0][..],
        &[0x66, 0xea, 0, 0x20, 0x43, 0],
        &[0xff, 0x2f],
        &[0x66, 0xff, 0x2f],
    ] {
        for (resident, entries) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut engine = EngineInstance::new(8, 0x464152).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(0x1000, bytes.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                engine.arena_mut().unwrap()[144..148]
                    .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            }
            let before = engine.arena()[..140].to_vec();
            match (resident, entries) {
                (false, false) => engine.compile(1).map(|_| ()),
                (false, true) => engine.compile_entries(1, 0).map(|_| ()),
                (true, false) => engine.compile_resident(1).map(|_| ()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|_| ()),
            }
            .unwrap();
            assert_eq!(&engine.arena()[..140], before);
        }
    }
}

#[test]
fn typed_far_targets_keep_offsets_selectors_segments_and_address_size() {
    use ring3_engine::cpu::x86::{
        FarWidth, SegmentRegister,
        ir::{AddressSize, FarPointer, Operation},
    };
    for (bytes, expected) in [
        (
            &[0xea, 0x78, 0x56, 0x34, 0x12, 0x47, 0][..],
            Operation::FarJump {
                width: FarWidth::Dword,
                target: FarPointer::Immediate {
                    offset: 0x12345678,
                    selector: 0x47,
                },
            },
        ),
        (
            &[0x66, 0xea, 0x78, 0x56, 0x47, 0][..],
            Operation::FarJump {
                width: FarWidth::Word,
                target: FarPointer::Immediate {
                    offset: 0x5678,
                    selector: 0x47,
                },
            },
        ),
    ] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0), bytes).unwrap();
        assert_eq!(
            *decode_one(&memory, GuestAddress(0)).unwrap().operation(),
            expected
        );
    }
    for (bytes, segment, size) in [
        (
            &[0xff, 0x6d, 0][..],
            SegmentRegister::Ss,
            AddressSize::Bits32,
        ),
        (
            &[0x64, 0xff, 0x2f][..],
            SegmentRegister::Fs,
            AddressSize::Bits32,
        ),
        (
            &[0x67, 0xff, 0x2a][..],
            SegmentRegister::Ss,
            AddressSize::Bits16,
        ),
    ] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0), bytes).unwrap();
        let Operation::FarJump {
            width: FarWidth::Dword,
            target: FarPointer::Memory(address),
        } = *decode_one(&memory, GuestAddress(0)).unwrap().operation()
        else {
            panic!("typed memory far pointer")
        };
        assert_eq!(address.resolved_segment(), segment);
        assert_eq!(address.address_size, size);
    }
}

#[test]
fn standalone_preparation_stays_closed_without_segment_authority() {
    use ring3_engine::cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, InstructionError, prepare_entry_region,
        prepare_region,
    };
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for bytes in [&[0xea, 0, 0x20, 0, 0, 0x43, 0][..], &[0xff, 0x2f]] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let expected = Some(CompileError::Instruction {
            pc: GuestAddress(0x1000),
            cause: InstructionError::BackendUnsupported,
        });
        assert_eq!(
            prepare_region(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default()
            )
            .err(),
            expected
        );
        assert_eq!(
            prepare_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).err(),
            expected
        );
    }
}
