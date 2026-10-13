use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        x86::decode::{DecodeError, decode_one},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, HostError},
};

const FORMS: &[(u8, u8, u32, bool)] = &[
    (1, 0xd8, 8, false),
    (1, 0xd9, 9, false),
    (1, 0xda, 10, false),
    (1, 0xdc, 11, false),
    (1, 0xdd, 12, false),
    (1, 0xde, 13, false),
    (1, 0xe0, 14, false),
    (1, 0xe3, 15, false),
    (1, 0xe8, 16, false),
    (1, 0xe9, 17, false),
    (1, 0xea, 18, false),
    (1, 0xec, 19, false),
    (1, 0xed, 20, false),
    (1, 0xee, 21, false),
    (2, 0x08, 29, false),
    (2, 0x09, 30, false),
    (2, 0x0a, 31, false),
    (2, 0x1c, 32, true),
    (2, 0x1d, 33, true),
    (2, 0x1e, 34, true),
    (2, 0x38, 37, false),
    (2, 0x39, 38, false),
    (2, 0x3a, 39, false),
    (2, 0x3b, 40, false),
    (2, 0x3c, 41, false),
    (2, 0x3d, 42, false),
    (2, 0x3e, 43, false),
    (2, 0x3f, 44, false),
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

fn encoding(map: u8, opcode: u8, unary: bool, wide: bool, w: bool, memory: bool) -> [u8; 5] {
    let vvvv = if unary { 15 } else { 13 };
    [
        0xc4,
        0xe0 | map,
        vvvv << 3 | if wide { 4 } else { 0 } | 1 | if w { 0x80 } else { 0 },
        opcode,
        if memory { 0x08 } else { 0xcb },
    ]
}

#[test]
fn remaining_wide_integer_encodings_stay_private() {
    for &(map, opcode, _, unary) in FORMS {
        for w in [false, true] {
            for memory in [false, true] {
                let bytes = encoding(map, opcode, unary, true, w, memory);
                assert_eq!(
                    decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                    DecodeError::Unsupported(UnsupportedFeature::Simd),
                    "{bytes:02x?}"
                );
                let xmm = encoding(map, opcode, unary, false, w, memory);
                assert!(
                    decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok(),
                    "{xmm:02x?}"
                );
            }
        }
    }
}

#[test]
fn malformed_prefixes_and_unary_vvvv_are_rejected_before_the_gate() {
    for &(map, opcode, _, unary) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let mut bytes = vec![prefix];
            bytes.extend(encoding(map, opcode, unary, true, false, false));
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
        if unary {
            let mut bytes = encoding(map, opcode, true, true, false, false);
            bytes[2] &= !8;
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
        }
    }
}

#[test]
fn canonical_wide_descriptors_refuse_without_arena_or_ram_publication() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let mut before_ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();
    for &(_, _, kind, unary) in FORMS {
        let first = if unary { 0 } else { 2 << 3 };
        for operands in [1 | first | 3 << 6, 1 | first | 1 << 12] {
            // capability refusal must precede stale memory metadata or page access.
            engine.arena_mut().unwrap()[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.avx_execute(0x01000580 | kind, operands, 0, 0, 0, 2),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
            let mut after_ram = vec![0; 4096];
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
