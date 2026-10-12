use ring3_engine::{
    abi::x86::{SEGMENTS_SIZE, decode_segments, encode_segments},
    cpu::x86::{
        Register32, SegmentRegister, SegmentState,
        decode::decode_one,
        ir::{Location32, Operation},
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
fn segment_record_has_flat_defaults_and_explicit_fs_gs_bases() {
    let mut state = SegmentState::default();
    assert_eq!(state.selectors, [0x23, 0x1b, 0x23, 0x23, 0x3b, 0]);
    assert!(state.set_base(SegmentRegister::Fs, 0x1234_5678));
    assert!(state.set_base(SegmentRegister::Gs, 0xfedc_ba98));
    assert!(!state.set_base(SegmentRegister::Ds, 1));
    let mut bytes = [0; SEGMENTS_SIZE];
    encode_segments(&state, &mut bytes).unwrap();
    assert_eq!(decode_segments(&bytes).unwrap(), state);
    bytes[28] = 1;
    assert!(decode_segments(&bytes).is_err());
    bytes[28] = 0;
    bytes[32] = 1;
    assert!(decode_segments(&bytes).is_err());
}

#[test]
fn mov_from_every_segment_distinguishes_word_parent_merge_from_dword_zero_extension() {
    for (index, segment) in [
        SegmentRegister::Es,
        SegmentRegister::Cs,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ]
    .into_iter()
    .enumerate()
    {
        let modrm = 0xc0 + ((index as u8) << 3);
        assert_eq!(
            decode(&[0x8c, modrm]),
            Operation::ReadSegment {
                segment,
                destination: Location32::Register(Register32::Eax),
                word: false,
            }
        );
        assert_eq!(
            decode(&[0x66, 0x8c, modrm]),
            Operation::ReadSegment {
                segment,
                destination: Location32::Register(Register32::Eax),
                word: true,
            }
        );
        match decode(&[0x8c, (index as u8) << 3]) {
            Operation::ReadSegment {
                segment: observed,
                destination: Location32::Memory(_),
                word: true,
            } => assert_eq!(observed, segment),
            operation => panic!("unexpected memory segment read {operation:?}"),
        }
    }
}

#[test]
fn segment_push_read_forms_compile_in_both_owners_without_descriptor_lookup() {
    for opcode in [
        vec![0x06],
        vec![0x0e],
        vec![0x16],
        vec![0x1e],
        vec![0x0f, 0xa0],
        vec![0x0f, 0xa8],
    ] {
        for word in [false, true] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend_from_slice(&opcode);
            assert!(
                matches!(decode(&code), Operation::PushSegment { word: observed, .. } if observed == word)
            );
            code.extend_from_slice(&[0xeb, 0xfe]);
            for resident in [false, true] {
                let mut engine = EngineInstance::new(1, 0x5347).unwrap();
                engine.map(0x1000, 1, 7).unwrap();
                engine.arena_mut().unwrap()[140..140 + code.len()].copy_from_slice(&code);
                engine.upload(0x1000, code.len() as u32).unwrap();
                engine.protect(0x1000, 1, 4).unwrap();
                let input = engine.arena_mut().unwrap();
                input[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
                input[144..148].copy_from_slice(&(code.len() as u32).to_le_bytes());
                if resident {
                    engine.compile_resident(1).unwrap();
                } else {
                    engine.compile(1).unwrap();
                }
            }
        }
    }
}

#[test]
fn descriptor_load_forms_are_typed_without_assuming_current_selector_equality() {
    for (bytes, segment) in [
        (vec![0x8e, 0xe0], SegmentRegister::Fs),
        (vec![0x66, 0x8e, 0xe8], SegmentRegister::Gs),
    ] {
        assert_eq!(
            decode(&bytes),
            Operation::LoadSegment {
                segment,
                source: ring3_engine::cpu::x86::ir::Value32::Register(Register32::Eax)
            }
        );
    }
    assert_eq!(
        decode(&[0x0f, 0xa1]),
        Operation::PopSegment {
            segment: SegmentRegister::Fs,
            word: false
        }
    );
    assert_eq!(
        decode(&[0x66, 0x0f, 0xa9]),
        Operation::PopSegment {
            segment: SegmentRegister::Gs,
            word: true
        }
    );
    for (opcode, segment) in [
        (0xb4, SegmentRegister::Fs),
        (0xb5, SegmentRegister::Gs),
        (0xb2, SegmentRegister::Ss),
    ] {
        match decode(&[0x0f, opcode, 0x00]) {
            Operation::LoadFarPointer {
                segment: observed,
                destination: Register32::Eax,
                word: false,
                ..
            } => assert_eq!(observed, segment),
            operation => panic!("unexpected far-pointer load {operation:?}"),
        }
    }
}
