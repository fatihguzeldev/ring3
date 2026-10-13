use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        x86::{SIMD_XMM_OFFSET, YMM_REGISTERS_OFFSET},
    },
    cpu::x86::{
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::{EngineInstance, StoreCompletion},
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

#[test]
fn avx2_integer_encodings_bind_to_the_existing_floating_semantics() {
    for (integer, floating) in OPCODES {
        for w in [0, 0x80] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe1, 0x6d | w, integer, operand];
                let memory = code(&bytes);
                let before = memory
                    .snapshot_code(GuestAddress(0x1000), bytes.len())
                    .unwrap();
                let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                let alias = decode_one(
                    &code(&[0xc4, 0xe1, 0x6c | w, floating, operand]),
                    GuestAddress(0x1000),
                )
                .unwrap();
                assert!(matches!(decoded.operation(), Operation::Avx(_)));
                assert_eq!(decoded.operation(), alias.operation(), "{bytes:02x?}");
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert!(memory.is_code_current(decoded.code_snapshot()));
                assert!(memory.is_code_current(&before));
            }
        }
    }
}

#[test]
fn invalid_prefixes_are_rejected_before_the_capability_gate() {
    for (integer, _) in OPCODES {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe1, 0x6d, integer, 0xcb];
            assert_eq!(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "{bytes:02x?}"
            );
        }
    }
}

#[test]
fn avx1_integer_xmm_and_floating_ymm_aliases_remain_admitted() {
    for (integer, floating) in OPCODES {
        let integer =
            decode_one(&code(&[0xc5, 0xe9, integer, 0xcb]), GuestAddress(0x1000)).unwrap();
        let floating_xmm =
            decode_one(&code(&[0xc5, 0xe8, floating, 0xcb]), GuestAddress(0x1000)).unwrap();
        assert_eq!(integer.operation(), floating_xmm.operation());
        assert!(matches!(integer.operation(), Operation::Avx(_)));
        for w in [0, 0x80] {
            let floating_ymm = decode_one(
                &code(&[0xc4, 0xe1, 0x6c | w, floating, 0xcb]),
                GuestAddress(0x1000),
            )
            .unwrap();
            assert!(matches!(floating_ymm.operation(), Operation::Avx(_)));
        }
    }
}

#[test]
fn shared_ymm_descriptors_remain_available_to_avx1_helpers() {
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
    for (kind, lower, upper) in [
        (2, 0x30, 0x0a),
        (4, 0x03, 0x05),
        (6, 0xf3, 0xaf),
        (8, 0xc3, 0xa5),
    ] {
        let arena = engine.arena_mut().unwrap();
        for (register, low, high) in [(1, 0xf0, 0xaa), (2, 0x33, 0x0f)] {
            arena[SIMD_OFFSET + SIMD_XMM_OFFSET + register * 16
                ..SIMD_OFFSET + SIMD_XMM_OFFSET + (register + 1) * 16]
                .fill(low);
            arena[YMM_OFFSET + YMM_REGISTERS_OFFSET + register * 16
                ..YMM_OFFSET + YMM_REGISTERS_OFFSET + (register + 1) * 16]
                .fill(high);
        }
        let mut expected = engine.arena().to_vec();
        expected[SIMD_OFFSET + SIMD_XMM_OFFSET..SIMD_OFFSET + SIMD_XMM_OFFSET + 16].fill(lower);
        expected[YMM_OFFSET + YMM_REGISTERS_OFFSET..YMM_OFFSET + YMM_REGISTERS_OFFSET + 16]
            .fill(upper);
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
        expected[HELPER_OFFSET..HELPER_OFFSET + 12]
            .copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&0x244cd7_u32.to_le_bytes());
        assert_eq!(
            engine.avx_execute(0x01000200 | kind, 1 << 3 | 2 << 6, 0, 0, 0, 0x244cd7),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected, "shared descriptor kind {kind}");
        let mut after_ram = vec![0; before_ram.len()];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x1000), &mut after_ram)
            .unwrap();
        assert_eq!(after_ram, before_ram);
        assert_eq!(
            engine.arena(),
            expected,
            "RAM observation preserves full frame"
        );
    }
}
