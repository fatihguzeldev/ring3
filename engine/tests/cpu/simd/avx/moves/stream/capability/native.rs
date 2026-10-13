use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

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
fn ymm_stream_encoding_refuses_without_gating_the_shared_avx1_aligned_load() {
    for destination in 0..8 {
        for w in [0, 0x80] {
            let bytes = [0xc4, 0xe2, 0x7d | w, 0x2a, destination << 3];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::Unsupported(UnsupportedFeature::Simd)
            );
            for operand in [destination << 3, 0xc0 | destination << 3 | 3] {
                let avx1 = [0xc4, 0xe1, 0x7d | w, 0x6f, operand];
                let decoded = decode_one(&code(&avx1), GuestAddress(0x1000)).unwrap();
                assert!(matches!(decoded.operation(), Operation::Avx(_)));
                assert_eq!(decoded.length(), 5);
            }
        }
    }
}

#[test]
fn stream_memory_only_and_reserved_encoding_rules_precede_the_gate() {
    for control in [0x79, 0x7d, 0xf9, 0xfd] {
        let valid = [0xc4, 0xe2, control, 0x2a, 0x08];
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, valid[0], valid[1], valid[2], valid[3], valid[4]];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        for reserved in 0..15 {
            let mut bytes = valid;
            bytes[2] = bytes[2] & !0x78 | reserved << 3;
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        let mut register = valid;
        register[4] = 0xcb;
        assert_eq!(
            decode_one(&code(&register), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn xmm_stream_keeps_its_existing_width_and_ignored_w_contract() {
    for destination in 0..8 {
        let a = [0xc4, 0xe2, 0x79, 0x2a, destination << 3];
        let b = [0xc4, 0xe2, 0xf9, 0x2a, destination << 3];
        let first = decode_one(&code(&a), GuestAddress(0x1000)).unwrap();
        let second = decode_one(&code(&b), GuestAddress(0x1000)).unwrap();
        assert_eq!(first.operation(), second.operation());
        assert!(matches!(first.operation(), Operation::Avx(_)));
        assert_eq!(first.length(), 5);
    }
}
