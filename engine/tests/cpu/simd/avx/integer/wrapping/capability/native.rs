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

const FORMS: [(u8, u32); 8] = [
    (0xfc, 26),
    (0xfd, 27),
    (0xfe, 28),
    (0xd4, 7),
    (0xf8, 22),
    (0xf9, 23),
    (0xfa, 24),
    (0xfb, 25),
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

fn ram(engine: &mut EngineInstance) -> Vec<u8> {
    let saved = engine.arena().to_vec();
    let mut value = Vec::with_capacity(4096);
    for offset in (0..4096).step_by(512) {
        engine.cpu_read(0x1000 + offset, 512, 1).unwrap();
        assert_eq!(
            &engine.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20],
            &[0; 4]
        );
        value.extend_from_slice(&engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512]);
    }
    engine.arena_mut().unwrap().copy_from_slice(&saved);
    value
}

#[test]
fn wide_wrapping_guest_encodings_bind_exact_wig_width_and_capture() {
    for (opcode, _) in FORMS {
        for w in [0, 0x80] {
            for operand in [0xcb, 0x08] {
                let bytes = [0xc4, 0xe1, 0x6d | w, opcode, operand];
                let memory = code(&bytes);
                let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                let mut canonical = bytes;
                canonical[2] &= !0x80;
                let canonical = decode_one(&code(&canonical), GuestAddress(0x1000)).unwrap();
                assert!(matches!(decoded.operation(), Operation::Avx(_)));
                assert_eq!(decoded.operation(), canonical.operation(), "{bytes:02x?}");
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert!(memory.is_code_current(decoded.code_snapshot()));
                let mut xmm = bytes;
                xmm[2] &= !4;
                let xmm = decode_one(&code(&xmm), GuestAddress(0x1000)).unwrap();
                assert_ne!(decoded.operation(), xmm.operation());
            }
            let xmm = [0xc4, 0xe1, 0x69 | w, opcode, 0xcb];
            assert!(decode_one(&code(&xmm), GuestAddress(0x1000)).is_ok());
        }
    }
}

#[test]
fn malformed_prefixes_are_rejected_before_the_public_capability_gate() {
    for (opcode, _) in FORMS {
        for prefix in [0x66, 0xf0, 0xf2, 0xf3] {
            let bytes = [prefix, 0xc4, 0xe1, 0x6d, opcode, 0xcb];
            crate::support_fetch::assert_decode_contract(
                decode_one(&code(&bytes), GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding,
            );
        }
    }
}

#[test]
fn canonical_wide_helpers_publish_registers_and_reject_real_malformed_inputs() {
    let mut engine = EngineInstance::new(1, 0x57524150).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let before_ram = ram(&mut engine);
    for (_, kind) in FORMS {
        let arena = engine.arena_mut().unwrap();
        arena[TRANSFER_OFFSET + 544..TRANSFER_OFFSET + 568].fill(0xff);
        arena[SIMD_OFFSET + SIMD_XMM_OFFSET + 16..SIMD_OFFSET + SIMD_XMM_OFFSET + 32].fill(0x91);
        arena[YMM_OFFSET + YMM_REGISTERS_OFFSET + 16..YMM_OFFSET + YMM_REGISTERS_OFFSET + 32]
            .fill(0x35);
        let mut expected = arena.to_vec();
        expected[SIMD_OFFSET + SIMD_XMM_OFFSET + 16..SIMD_OFFSET + SIMD_XMM_OFFSET + 32].fill(0);
        expected[YMM_OFFSET + YMM_REGISTERS_OFFSET + 16..YMM_OFFSET + YMM_REGISTERS_OFFSET + 32]
            .fill(0);
        expected[HELPER_OFFSET..HELPER_OFFSET + 40].fill(0);
        expected[HELPER_OFFSET..HELPER_OFFSET + 12]
            .copy_from_slice(&[b'R', b'3', b'M', b'H', 7, 0, 1, 0, 40, 0, 0, 0]);
        expected[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 544].fill(0);
        expected[TRANSFER_OFFSET + 516..TRANSFER_OFFSET + 520]
            .copy_from_slice(&2_u32.to_le_bytes());
        // register requests ignore stale R3AM and publish both complete YMM halves.
        assert_eq!(
            engine.avx_execute(0x0100_0580 | kind, 1 | 2 << 3 | 3 << 6, 0, 0, 0, 2),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected);
        assert_eq!(ram(&mut engine), before_ram);
        assert_eq!(engine.arena(), expected);

        // canonical memory requests require a valid request record, with canonical vector state.
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.avx_execute(0x0100_0580 | kind, 1 | 2 << 3 | 1 << 12, 0, 0, 0, 2),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(ram(&mut engine), before_ram);
        assert_eq!(engine.arena(), before);
    }
    engine.arena_mut().unwrap()[YMM_OFFSET + 12] = 1;
    let before = engine.arena().to_vec();
    // malformed operands still precede malformed host state after activation.
    assert_eq!(
        engine.avx_execute(0x0100_0580 | 26, 1 | 2 << 3 | 3 << 6 | 1 << 9, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&mut engine), before_ram);
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.avx_execute(0x0100_0580 | 26, 1 | 2 << 3 | 3 << 6, 0, 0, 0, 2),
        Err(HostError::Infrastructure)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(ram(&mut engine), before_ram);
    assert_eq!(engine.arena(), before);
}
