use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

const OPCODES: [(u8, u8); 4] = [(0xdb, 0x54), (0xdf, 0x55), (0xeb, 0x56), (0xef, 0x57)];

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

fn assert_alias(integer: &[u8], floating: &[u8]) {
    let memory = code(integer);
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    let original = decode_one(&code(floating), GuestAddress(0x1000)).unwrap();
    assert!(matches!(decoded.operation(), Operation::Avx(_)));
    assert_eq!(decoded.operation(), original.operation(), "{integer:02x?}");
    assert_eq!(usize::from(decoded.length()), integer.len());
    assert_eq!(
        decoded.next_pc(),
        GuestAddress(0x1000 + integer.len() as u32)
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn all_four_ymm_aliases_preserve_operand_roles_and_ignored_w() {
    for (integer, floating) in OPCODES {
        for register in 0..8 {
            for (destination, first, second) in [
                (register, (register + 1) % 8, (register + 2) % 8),
                (register, register, (register + 1) % 8),
                (register, (register + 1) % 8, register),
                (register, (register + 1) % 8, (register + 1) % 8),
                (register, register, register),
            ] {
                let control = 0x85 | (15 - first) << 3;
                let modrm = 0xc0 | destination << 3 | second;
                assert_alias(
                    &[0xc5, control, integer, modrm],
                    &[0xc5, control & !3, floating, modrm],
                );
                for w in [0, 0x80] {
                    let control = control & 0x7f | w;
                    assert_alias(
                        &[0xc4, 0xe1, control, integer, modrm],
                        &[0xc4, 0xe1, control & !3, floating, modrm],
                    );
                }
            }
        }
    }
}

#[test]
fn all_four_memory_aliases_capture_exact_addresses_without_reading_data() {
    for (integer, floating) in OPCODES {
        for w in [0, 0x80] {
            for (prefix, addressing) in [
                (None, &[0x08][..]),
                (None, &[0x4c, 0x24, 0x7f][..]),
                (None, &[0x0d, 0, 0x50, 0, 0][..]),
                (Some(0x64), &[0x08][..]),
                (Some(0x67), &[0x0a][..]),
            ] {
                let mut integer_bytes = prefix.into_iter().collect::<Vec<_>>();
                integer_bytes.extend([0xc4, 0xe1, 0x6d | w, integer]);
                integer_bytes.extend(addressing);
                let mut floating_bytes = integer_bytes.clone();
                let vex_control = usize::from(prefix.is_some()) + 2;
                floating_bytes[vex_control] &= !3;
                floating_bytes[vex_control + 1] = floating;
                assert_alias(&integer_bytes, &floating_bytes);
            }
        }
    }
}

#[test]
fn forbidden_legacy_prefixes_remain_encoding_errors() {
    for (integer, _) in OPCODES {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe1, 0x6d, integer, 0xcb];
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn existing_xmm_integer_and_ymm_floating_forms_stay_admitted() {
    for (integer, floating) in OPCODES {
        assert_alias(&[0xc5, 0xe9, integer, 0xcb], &[0xc5, 0xe8, floating, 0xcb]);
        assert_alias(&[0xc5, 0xec, floating, 0xcb], &[0xc5, 0xec, floating, 0xcb]);
    }
}
