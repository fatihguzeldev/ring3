use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_XMM_OFFSET, YMM_REGISTERS_OFFSET},
    },
    cpu::{
        UnsupportedFeature,
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, StoreCompletion},
};

const FAMILIES: [(u8, u8, u8, bool); 4] = [
    (2, 0x5a, 0x1a, false),
    (3, 0x38, 0x18, true),
    (3, 0x39, 0x19, false),
    (3, 0x46, 0x06, true),
];

fn instruction(map: u8, opcode: u8, source: bool, modrm: u8) -> Vec<u8> {
    let mut bytes = vec![
        0xc4,
        0xe0 | map,
        if source { 0x6d } else { 0x7d },
        opcode,
        modrm,
    ];
    if map == 3 {
        bytes.push(0xff);
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

#[test]
fn avx2_block_encodings_remain_outside_the_public_profile() {
    for (map, integer, _, source) in FAMILIES {
        for modrm in [0x08, 0xcb] {
            if map == 2 && modrm == 0xcb {
                continue;
            }
            for prefix in [None, Some(0x36), Some(0x67)] {
                let mut bytes = instruction(map, integer, source, modrm);
                if let Some(prefix) = prefix {
                    bytes.insert(0, prefix);
                }
                let memory = executable(&bytes);
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

#[test]
fn reserved_encodings_are_rejected_before_the_capability_gate() {
    for (map, integer, _, source) in FAMILIES {
        let bytes = instruction(map, integer, source, 0x08);
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
        if !source {
            let mut vvvv = bytes.clone();
            vvvv[2] &= !8;
            invalid.push(vvvv);
        }
        if map == 2 {
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
fn all_seven_avx1_block_forms_remain_admitted() {
    for (map, _, floating, source) in FAMILIES {
        for modrm in [0x08, 0xcb] {
            if map == 2 && modrm == 0xcb {
                continue;
            }
            let bytes = instruction(map, floating, source, modrm);
            let memory = executable(&bytes);
            let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
            assert!(matches!(decoded.operation(), Operation::Avx(_)));
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn shared_block_descriptors_remain_available_to_avx1_helpers() {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0x55);
    engine.upload(0x5000, 16).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    for (kind, operands, immediate, low, high, length) in [
        (4, 1 << 12, 0, 0x55, 0x55, 16_u32),
        (5, 1 << 3 | 2 << 6, 0x13, 0x44, 0x22, 0),
        (6, 1 << 3 | 2 << 6, 1, 0x11, 0x33, 0),
        (7, 1 << 6, 1, 0x22, 0, 0),
    ] {
        let arena = engine.arena_mut().unwrap();
        for (register, low, high) in [(1, 0x11, 0x22), (2, 0x33, 0x44)] {
            arena[SIMD_OFFSET + SIMD_XMM_OFFSET + register * 16
                ..SIMD_OFFSET + SIMD_XMM_OFFSET + (register + 1) * 16]
                .fill(low);
            arena[YMM_OFFSET + YMM_REGISTERS_OFFSET + register * 16
                ..YMM_OFFSET + YMM_REGISTERS_OFFSET + (register + 1) * 16]
                .fill(high);
        }
        let metadata = &mut arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568];
        metadata.fill(0);
        metadata[..12].copy_from_slice(&[b'R', b'3', b'A', b'M', 1, 0, 1, 0, 24, 0, 0, 0]);
        metadata[16..20].copy_from_slice(&3_u32.to_le_bytes());
        metadata[20..24].copy_from_slice(&0x5000_u32.to_le_bytes());
        let mut expected = engine.arena().to_vec();
        expected[SIMD_OFFSET + SIMD_XMM_OFFSET..SIMD_OFFSET + SIMD_XMM_OFFSET + 16].fill(low);
        expected[YMM_OFFSET + YMM_REGISTERS_OFFSET..YMM_OFFSET + YMM_REGISTERS_OFFSET + 16]
            .fill(high);
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
        expected[HELPER_OFFSET..HELPER_OFFSET + 12]
            .copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
        expected[HELPER_OFFSET + 36..HELPER_OFFSET + 40].copy_from_slice(&length.to_le_bytes());
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&0x244cd7_u32.to_le_bytes());
        assert_eq!(
            engine.avx_execute(0x01000e00 | kind, operands, immediate, 0, 0, 0x244cd7),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected, "shared descriptor kind {kind}");
    }
}
