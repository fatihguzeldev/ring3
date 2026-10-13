use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn integer_masked_encodings_remain_outside_the_public_profile() {
    for opcode in [0x8c, 0x8e] {
        for control in [0x69, 0x6d, 0xe9, 0xed] {
            for modrm in [0x08, 0x18, 0x38] {
                for prefix in [None, Some(0x36), Some(0x67)] {
                    let mut bytes = vec![0xc4, 0xe2, control, opcode, modrm];
                    if let Some(prefix) = prefix {
                        bytes.insert(0, prefix);
                    }
                    let memory = code(&bytes);
                    let before = memory
                        .snapshot_code(GuestAddress(0x1000), bytes.len())
                        .unwrap();
                    assert_eq!(
                        decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
                        DecodeError::Unsupported(UnsupportedFeature::Simd),
                        "{bytes:02x?}"
                    );
                    assert!(memory.is_code_current(&before));
                }
            }
        }
    }
}

#[test]
fn forbidden_prefixes_and_register_substitutions_stay_invalid_before_the_gate() {
    for opcode in [0x8c, 0x8e] {
        for control in [0x69, 0x6d, 0xe9, 0xed] {
            for prefix in [0xf0, 0x66, 0xf2, 0xf3] {
                let bytes = [prefix, 0xc4, 0xe2, control, opcode, 0x08];
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::InvalidEncoding
                );
            }
            let bytes = [0xc4, 0xe2, control, opcode, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn shared_float_encodings_remain_available_with_their_original_w0_rule() {
    for opcode in [0x2c, 0x2d, 0x2e, 0x2f] {
        for control in [0x69, 0x6d] {
            let bytes = [0xc4, 0xe2, control, opcode, 0x08];
            let memory = code(&bytes);
            let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert!(memory.is_code_current(decoded.code_snapshot()));
            let mut invalid = bytes;
            invalid[2] |= 0x80;
            assert_eq!(
                decode_one(&code(&invalid), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}
