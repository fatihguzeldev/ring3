use ring3_engine::{
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[derive(Clone, Copy)]
struct Family {
    map: u8,
    integer: u8,
    floating: u8,
    immediate: bool,
}

const BROADCAST: Family = Family {
    map: 2,
    integer: 0x5a,
    floating: 0x1a,
    immediate: false,
};
const INSERT: Family = Family {
    map: 3,
    integer: 0x38,
    floating: 0x18,
    immediate: true,
};
const EXTRACT: Family = Family {
    map: 3,
    integer: 0x39,
    floating: 0x19,
    immediate: true,
};
const PERMUTE: Family = Family {
    map: 3,
    integer: 0x46,
    floating: 0x06,
    immediate: true,
};

fn instruction(family: Family, first: Option<u8>, modrm: &[u8], immediate: u8) -> Vec<u8> {
    let encoded_first = first.map_or(15, |register| 15 - register);
    let mut bytes = vec![
        0xc4,
        0xe0 | family.map,
        5 | encoded_first << 3,
        family.integer,
    ];
    bytes.extend(modrm);
    if family.immediate {
        bytes.push(immediate);
    }
    bytes
}

fn executable(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn assert_alias(integer: &[u8], floating: &[u8]) {
    let memory = executable(integer);
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    let reference = decode_one(&executable(floating), GuestAddress(0x1000)).unwrap();
    assert!(matches!(decoded.operation(), Operation::Avx(_)));
    assert_eq!(decoded.operation(), reference.operation(), "{integer:02x?}");
    assert_eq!(usize::from(decoded.length()), integer.len());
    assert_eq!(
        decoded.next_pc(),
        GuestAddress(0x1000 + integer.len() as u32)
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

fn assert_family_alias(family: Family, first: Option<u8>, modrm: &[u8], immediate: u8) {
    let integer = instruction(family, first, modrm, immediate);
    let mut floating = integer.clone();
    floating[3] = family.floating;
    assert_alias(&integer, &floating);
}

#[test]
fn integer_register_forms_preserve_all_roles_and_immediates() {
    for register in 0..8 {
        for (destination, first, second) in [
            (register, (register + 1) % 8, (register + 2) % 8),
            (register, register, (register + 1) % 8),
            (register, (register + 1) % 8, register),
            (register, (register + 1) % 8, (register + 1) % 8),
            (register, register, register),
        ] {
            for family in [INSERT, PERMUTE] {
                for immediate in [0, 1, 2, 0x08, 0x80, 0xff] {
                    assert_family_alias(
                        family,
                        Some(first),
                        &[0xc0 | destination << 3 | second],
                        immediate,
                    );
                }
            }
        }
        for destination in [register, (register + 1) % 8] {
            for immediate in [0, 1, 0xfe, 0xff] {
                assert_family_alias(
                    EXTRACT,
                    None,
                    &[0xc0 | register << 3 | destination],
                    immediate,
                );
            }
        }
    }
    for immediate in 0..=255 {
        for family in [INSERT, PERMUTE] {
            assert_family_alias(family, Some(2), &[0xcb], immediate);
        }
        assert_family_alias(EXTRACT, None, &[0xd9], immediate);
    }
}

#[test]
fn integer_memory_forms_bind_addresses_without_data_access() {
    for family in [BROADCAST, INSERT, EXTRACT, PERMUTE] {
        let first = matches!(family.integer, 0x38 | 0x46).then_some(2);
        for (prefix, modrm) in [
            (None, &[0x08][..]),
            (None, &[0x4c, 0x24, 0x7f][..]),
            (None, &[0x0d, 0, 0x50, 0, 0][..]),
            (Some(0x64), &[0x08][..]),
            (Some(0x36), &[0x08][..]),
            (Some(0x67), &[0x0a][..]),
        ] {
            let mut integer = instruction(family, first, modrm, 0xa5);
            let mut floating = integer.clone();
            floating[3] = family.floating;
            if let Some(prefix) = prefix {
                integer.insert(0, prefix);
                floating.insert(0, prefix);
            }
            assert_alias(&integer, &floating);
        }
    }
}

#[test]
fn reserved_encodings_stay_invalid() {
    for family in [BROADCAST, INSERT, EXTRACT, PERMUTE] {
        let first = matches!(family.integer, 0x38 | 0x46).then_some(2);
        let bytes = instruction(family, first, &[0x08], 0);
        let mut invalid = Vec::new();
        for prefix in [0xf0, 0x66, 0xf2, 0xf3] {
            invalid.push([&[prefix][..], &bytes].concat());
        }
        let mut w1 = bytes.clone();
        w1[2] |= 0x80;
        invalid.push(w1);
        let mut l0 = bytes.clone();
        l0[2] &= !4;
        invalid.push(l0);
        if first.is_none() {
            let mut reserved_source = bytes.clone();
            reserved_source[2] &= !8;
            invalid.push(reserved_source);
        }
        if family.integer == BROADCAST.integer {
            let mut register_source = bytes.clone();
            register_source[4] = 0xcb;
            invalid.push(register_source);
        }
        for bytes in invalid {
            assert_eq!(
                decode_one(&executable(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn floating_block_forms_stay_admitted() {
    for family in [BROADCAST, INSERT, EXTRACT, PERMUTE] {
        let first = matches!(family.integer, 0x38 | 0x46).then_some(2);
        for modrm in [&[0x08][..], &[0xcb][..]] {
            if family.integer == BROADCAST.integer && modrm[0] & 0xc0 == 0xc0 {
                continue;
            }
            let mut bytes = instruction(family, first, modrm, 0xff);
            bytes[3] = family.floating;
            let decoded = decode_one(&executable(&bytes), GuestAddress(0x1000)).unwrap();
            assert!(matches!(decoded.operation(), Operation::Avx(_)));
            assert_eq!(usize::from(decoded.length()), bytes.len());
        }
    }
}
