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
    process::{EngineInstance, HostError, StoreCompletion},
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
fn byte_masks_keep_exact_wig_bindings_and_distinct_widths() {
    for destination in 0..8 {
        for source in 0..8 {
            for w in [0, 0x80] {
                let mut bytes = [0xc4, 0xe1, 0x7d | w, 0xd7, 0xc0 | destination << 3 | source];
                let memory = code(&bytes);
                let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                let mut canonical = bytes;
                canonical[2] &= !0x80;
                let canonical = decode_one(&code(&canonical), GuestAddress(0x1000)).unwrap();
                assert!(matches!(decoded.operation(), Operation::Avx(_)));
                assert_eq!(decoded.operation(), canonical.operation());
                assert_eq!(decoded.length(), 5);
                assert_eq!(decoded.next_pc(), GuestAddress(0x1005));
                assert!(memory.is_code_current(decoded.code_snapshot()));
                bytes[2] &= !4;
                let xmm = decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap();
                assert_ne!(decoded.operation(), xmm.operation());
            }
        }
    }
}

#[test]
fn malformed_mask_encoding_precedes_the_public_capability_refusal() {
    for width in [0x79, 0x7d] {
        let valid = [0xc4, 0xe1, width, 0xd7, 0xc1];
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
        let mut memory = valid;
        memory[4] = 0;
        assert_eq!(
            decode_one(&code(&memory), GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
}

#[test]
fn wide_mask_helper_separates_malformed_requests_state_and_success() {
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
    let arena = engine.arena_mut().unwrap();
    arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
    arena[YMM_OFFSET + 12] = 1;
    let before = arena.to_vec();
    assert_eq!(
        engine.avx_execute(0x0100_0983, 1 | 3 << 6 | 1 << 9, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.avx_execute(0x0100_0983, 1 | 3 << 6, 0, 0, 0, 2),
        Err(HostError::Infrastructure)
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

    let arena = engine.arena_mut().unwrap();
    arena[YMM_OFFSET + 12] = 0;
    for byte in 0..32 {
        let value = if matches!(byte, 0 | 17 | 31) {
            0x80
        } else {
            0x7f
        };
        let at = if byte < 16 {
            SIMD_OFFSET + SIMD_XMM_OFFSET + 3 * 16 + byte
        } else {
            YMM_OFFSET + YMM_REGISTERS_OFFSET + 3 * 16 + byte - 16
        };
        arena[at] = value;
    }
    // stale address metadata is irrelevant to a valid register-only byte-mask request.
    let mut expected = arena.to_vec();
    expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
    expected[HELPER_OFFSET..HELPER_OFFSET + 12]
        .copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
    expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 516]
        .copy_from_slice(&0x8002_0001_u32.to_le_bytes());
    expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(
        engine.avx_execute(0x0100_0983, 1 | 3 << 6, 0, 0, 0, 2),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(engine.arena(), expected);
    let mut after_ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut after_ram)
        .unwrap();
    assert_eq!(after_ram, before_ram);
    assert_eq!(engine.arena(), expected);
}
