use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
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
fn all_fused_orders_shapes_and_memory_operands_admit_without_fetching_data() {
    let mut forms = 0;
    for order in [0x90, 0xa0, 0xb0] {
        for operation in 6..=15 {
            for double in [false, true] {
                let scalar = operation & 1 != 0 && operation >= 9;
                for wide in [false, true] {
                    if scalar && wide {
                        continue;
                    }
                    let control = 0x69 | if double { 0x80 } else { 0 } | if wide { 4 } else { 0 };
                    for modrm in [0xcb, 0x08] {
                        let bytes = [0xc4, 0xe2, control, order | operation, modrm];
                        let memory = code(&bytes);
                        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                        assert!(
                            matches!(decoded.operation(), Operation::Avx(_)),
                            "{bytes:02x?}"
                        );
                        assert_eq!(usize::from(decoded.length()), bytes.len());
                        assert_eq!(decoded.next_pc(), GuestAddress(0x1005));
                        assert!(memory.is_code_current(decoded.code_snapshot()));
                    }
                    forms += 1;
                }
            }
        }
    }
    assert_eq!(forms, 96);
}

#[test]
fn forbidden_fused_legacy_prefixes_remain_encoding_errors() {
    for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
        let bytes = [prefix, 0xc4, 0xe2, 0x69, 0x98, 0xcb];
        assert_eq!(
            decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}
