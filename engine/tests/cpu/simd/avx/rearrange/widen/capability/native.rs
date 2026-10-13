use ring3_engine::{
    abi::arena::{TRANSFER_OFFSET, YMM_OFFSET},
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

const FORMS: [(u8, u32); 12] = [
    (0x20, 16),
    (0x21, 17),
    (0x22, 18),
    (0x23, 19),
    (0x24, 20),
    (0x25, 21),
    (0x30, 23),
    (0x31, 24),
    (0x32, 25),
    (0x33, 26),
    (0x34, 27),
    (0x35, 28),
];

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

#[test]
fn ymm_widening_guest_encodings_remain_outside_the_public_profile() {
    for (opcode, _) in FORMS {
        for w in [0, 0x80] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe2, 0x7d | w, opcode, operand];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd)
                );
            }
            let xmm = [0xc4, 0xe2, 0x79 | w, opcode, 0xcb];
            assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
        }
    }
}

#[test]
fn reserved_encoding_is_rejected_before_the_public_capability_gate() {
    for (opcode, _) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe2, 0x7d, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        for reserved in 0..15 {
            let bytes = [0xc4, 0xe2, 5 | reserved << 3, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn canonical_wide_descriptors_refuse_before_reading_state_or_address_metadata() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for (_, kind) in FORMS {
        for operands in [1 | 3 << 6, 1 | 1 << 12] {
            let arena = engine.arena_mut().unwrap();
            arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            arena[YMM_OFFSET + 12] = 1;
            let before = arena.to_vec();
            assert_eq!(
                engine.avx_execute(0x01000680 | kind, operands, 0, 0, 0, 2),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            let mut after_ram = [0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(0x1000), &mut after_ram)
                .unwrap();
            assert_eq!(after_ram, before_ram);
            assert_eq!(engine.arena(), before);
        }
    }
}
