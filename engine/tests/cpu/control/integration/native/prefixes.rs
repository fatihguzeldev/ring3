use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        Register32, SegmentRegister,
        decode::decode_one,
        ir::{AddressSize, BranchTarget, EffectiveAddress, Operation, WordValue},
    },
    memory::{Access, GuestAddress},
    process::EngineInstance,
};

const CODE: u32 = 0x1000;

fn code(blocks: &[(u32, &[u8])]) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0x5052_4546_424e_4459).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    for &(pc, bytes) in blocks {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.protect(CODE, 1, 4).unwrap();
    engine
}

#[test]
fn word_address16_moffs_has_disp16_and_selected_segment_metadata() {
    for (bytes, segment, displacement, store) in [
        (&[0x66, 0x67, 0xa1, 0x10, 0x50][..], None, 0x5010, false),
        (&[0x66, 0x67, 0xa3, 0x10, 0x50][..], None, 0x5010, true),
        (
            &[0x64, 0x66, 0x67, 0xa1, 0x10, 0][..],
            Some(SegmentRegister::Fs),
            0x10,
            false,
        ),
        (
            &[0x65, 0x66, 0x67, 0xa3, 0x12, 0][..],
            Some(SegmentRegister::Gs),
            0x12,
            true,
        ),
    ] {
        let engine = code(&[(CODE, bytes)]);
        let before = engine.arena().to_vec();
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        let address = EffectiveAddress {
            displacement,
            segment,
            address_size: AddressSize::Bits16,
            ..Default::default()
        };
        let expected = if store {
            Operation::StoreWord {
                address,
                source: WordValue::Register(Register32::Eax),
            }
        } else {
            Operation::LoadWord {
                destination: Register32::Eax,
                address,
            }
        };
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        assert_eq!(engine.arena(), before);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0x5010), Access::Read)
                .is_err()
        );
    }
}

const CONTROL: [(u32, &[u8]); 6] = [
    (0x1000, &[0x64, 0x67, 0xe8, 0xf9, 0, 0, 0]),
    (0x1100, &[0x65, 0x67, 0xc2, 4, 0]),
    (0x1007, &[0xeb, 0xfe]),
    (0x1200, &[0x64, 0x67, 0x66, 0xe8, 0xfa, 0]),
    (0x1300, &[0x65, 0x67, 0x66, 0xc2, 6, 0]),
    (0x1206, &[0xeb, 0xfe]),
];

#[test]
fn prefixed_near_call_return_keeps_operand_width_target_and_consumed_length() {
    let engine = code(&CONTROL);
    let before = engine.arena().to_vec();
    for (index, expected) in [
        Operation::Call {
            target: BranchTarget::Direct(GuestAddress(0x1100)),
        },
        Operation::Return { stack_adjust: 4 },
        Operation::CallWord {
            target: GuestAddress(0x1300),
        },
        Operation::ReturnWord { stack_adjust: 6 },
    ]
    .into_iter()
    .enumerate()
    {
        let (pc, bytes) = CONTROL[[0, 1, 3, 4][index]];
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &expected);
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(pc + bytes.len() as u32));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
    }
    assert_eq!(engine.arena(), before);
}

#[test]
fn prefixed_near_control_compiles_explicit_and_entry_under_both_owners() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = code(&CONTROL);
            for (index, &(pc, bytes)) in CONTROL.iter().enumerate() {
                let at = TRANSFER_OFFSET + index * if entries { 4 } else { 8 };
                let arena = engine.arena_mut().unwrap();
                arena[at..at + 4].copy_from_slice(&pc.to_le_bytes());
                if !entries {
                    arena[at + 4..at + 8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                }
            }
            let before = engine.arena().to_vec();
            let id = match (resident, entries) {
                (false, false) => u64::from(engine.compile(CONTROL.len() as u32).unwrap()),
                (false, true) => {
                    u64::from(engine.compile_entries(CONTROL.len() as u32, 0).unwrap())
                }
                (true, false) => engine.compile_resident(CONTROL.len() as u32).unwrap().get(),
                (true, true) => engine
                    .compile_resident_entries(CONTROL.len() as u32, 0)
                    .unwrap()
                    .get(),
            };
            let bytes = if resident {
                engine.resident_bytes(id).unwrap()
            } else {
                engine.artifact_bytes().unwrap()
            };
            assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0x7080), Access::Read)
                    .is_err()
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(CODE), Access::Read)
                    .is_err()
            );
        }
    }
}
