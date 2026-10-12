use ring3_engine::{
    cpu::x86::{
        decode::decode_one,
        ir::Operation,
        simd::{self, SimdError, SimdOperand, SimdOperation, SimdRegister, SimdState},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn bytes(map: u8, opcode: u8, modrm: u8, immediate: Option<u8>) -> Vec<u8> {
    let mut bytes = vec![0x66, 0x0f, map, opcode, modrm];
    bytes.extend(immediate);
    bytes
}

fn decode(bytes: &[u8]) -> SimdOperation {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(usize::from(instruction.length()), bytes.len());
    let Operation::Simd(op) = instruction.operation() else {
        panic!("typed SIMD expected")
    };
    let (descriptor, operands, immediate) = op.descriptor();
    let restored = simd::decode_descriptor(descriptor, operands, immediate).unwrap();
    assert_eq!(restored.kind, op.kind);
    assert_eq!(restored.vector_bytes, op.vector_bytes);
    assert_eq!(restored.lane_bytes, op.lane_bytes);
    assert_eq!(restored.aligned, op.aligned);
    *op
}

fn run(op: SimdOperation, state: &mut SimdState) -> simd::SimdResult {
    simd::execute(
        op,
        state,
        &mut [[0x5a; 8]; 8],
        &mut [0; 16],
        0x5060_7080,
        0x1020_3040,
        0xffff_ffff,
    )
    .unwrap()
}

fn hex(text: &str) -> [u8; 16] {
    assert_eq!(text.len(), 32);
    core::array::from_fn(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
}

fn qwords(values: [u64; 2]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (i, value) in values.into_iter().enumerate() {
        bytes[i * 8..i * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn crypto_decodes_seven_legacy_forms_with_full_aligned_memory_and_roundtrips() {
    for (map, opcode, immediate, lane) in [
        (0x38, 0xdb, None, 1),
        (0x38, 0xdc, None, 1),
        (0x38, 0xdd, None, 1),
        (0x38, 0xde, None, 1),
        (0x38, 0xdf, None, 1),
        (0x3a, 0xdf, Some(0xab), 1),
        (0x3a, 0x44, Some(0xff), 8),
    ] {
        for destination in 0..8 {
            for source in 0..8 {
                let op = decode(&bytes(
                    map,
                    opcode,
                    0xc0 | destination << 3 | source,
                    immediate,
                ));
                assert_eq!(op.vector_bytes, 16);
                assert_eq!(op.lane_bytes, lane);
                assert_eq!(op.memory_bytes, 0);
                assert!(!op.scalar);
                assert_eq!(
                    op.destination,
                    SimdOperand::Register(SimdRegister::Xmm(destination))
                );
                assert_eq!(op.source, SimdOperand::Register(SimdRegister::Xmm(source)));
                assert_eq!(op.immediate, immediate);
            }
        }
        let op = decode(&bytes(map, opcode, 0x00, immediate));
        assert_eq!(op.memory_bytes, 16);
        assert!(op.aligned);
        assert!(op.reads_memory());
        assert!(!op.writes_memory());
    }
}

#[test]
fn crypto_aes_rounds_match_aes128_intermediate_values_without_numeric_side_effects() {
    for (opcode, input, key, expected) in [
        (
            0xdc,
            "00102030405060708090a0b0c0d0e0f0",
            "d6aa74fdd2af72fadaa678f1d6ab76fe",
            "89d810e8855ace682d1843d8cb128fe4",
        ),
        (
            0xdd,
            "bd6e7c3df2b5779e0b61216e8b10b689",
            "13111d7fe3944a17f307a78b4d2b30c5",
            "69c4e0d86a7b0430d8cdb78070b4c55a",
        ),
        (
            0xde,
            "7ad5fda789ef4e272bca100b3d9ff59f",
            "13aa29be9c8faff6f770f58000f7bf03",
            "54d990a16ba09ab596bbf40ea111702f",
        ),
        (
            0xdf,
            "6353e08c0960e104cd70b751bacad0e7",
            "000102030405060708090a0b0c0d0e0f",
            "00112233445566778899aabbccddeeff",
        ),
    ] {
        let mut state = SimdState {
            mxcsr: 0x003f,
            ..SimdState::default()
        };
        state.xmm[0] = hex(input);
        state.xmm[1] = hex(key);
        state.xmm[7] = [0xa5; 16];
        let result = run(decode(&bytes(0x38, opcode, 0xc1, None)), &mut state);
        assert_eq!(state.xmm[0], hex(expected), "opcode {opcode:02x}");
        assert_eq!(state.xmm[1], hex(key));
        assert_eq!(state.xmm[7], [0xa5; 16]);
        assert_eq!(state.mxcsr, 0x003f);
        assert_eq!(result.flags, 0xffff_ffff);
        assert_eq!(result.gpr, 0x1020_3040);
    }
    let source = hex("549932d1f08557681093ed9cbe2c974e");
    for modrm in [0xc1, 0xc0] {
        let mut state = SimdState::default();
        state.xmm[0] = if modrm == 0xc0 { source } else { [0xa5; 16] };
        state.xmm[1] = source;
        run(decode(&bytes(0x38, 0xdb, modrm, None)), &mut state);
        assert_eq!(state.xmm[0], hex("13aa29be9c8faff6f770f58000f7bf03"));
    }
}

#[test]
fn crypto_key_assist_uses_only_odd_source_words_and_every_round_constant_bit() {
    let source = hex("000102030405060708090a0b0c0d0e0f");
    for immediate in 0..=255 {
        let mut expected = hex("f26b6fc56b6fc5f2fed7ab76d7ab76fe");
        expected[4] ^= immediate;
        expected[12] ^= immediate;
        for alias in [false, true] {
            let mut state = SimdState {
                mxcsr: 0xffff,
                ..SimdState::default()
            };
            state.xmm[0] = if alias { source } else { [0x7f; 16] };
            state.xmm[1] = source;
            run(
                decode(&bytes(
                    0x3a,
                    0xdf,
                    if alias { 0xc0 } else { 0xc1 },
                    Some(immediate),
                )),
                &mut state,
            );
            assert_eq!(state.xmm[0], expected, "immediate {immediate:02x}");
            assert_eq!(state.mxcsr, 0xffff);
        }
    }
    let mut state = SimdState::default();
    state.xmm[1] = source;
    state.xmm[1][..4].fill(0xff);
    state.xmm[1][8..12].fill(0x80);
    run(decode(&bytes(0x3a, 0xdf, 0xc1, Some(1))), &mut state);
    assert_eq!(state.xmm[0], hex("f26b6fc56a6fc5f2fed7ab76d6ab76fe"));
}

#[test]
fn crypto_carryless_multiply_selects_halves_and_ignores_other_immediate_bits() {
    for immediate in 0..=255 {
        let mut state = SimdState {
            mxcsr: 0x003f,
            ..SimdState::default()
        };
        state.xmm[0] = qwords([1 << 63, 3]);
        state.xmm[1] = qwords([2, 0x8000_0000_0000_0001]);
        let expected = match immediate & 0x11 {
            0 => [0, 1],
            1 => [6, 0],
            0x10 => [0x8000_0000_0000_0000, 0x4000_0000_0000_0000],
            0x11 => [0x8000_0000_0000_0003, 1],
            _ => unreachable!(),
        };
        let result = run(
            decode(&bytes(0x3a, 0x44, 0xc1, Some(immediate))),
            &mut state,
        );
        assert_eq!(state.xmm[0], qwords(expected), "immediate {immediate:02x}");
        assert_eq!(state.mxcsr, 0x003f);
        assert_eq!(result.flags, 0xffff_ffff);
    }
    let op = decode(&bytes(0x3a, 0x44, 0xc1, Some(0)));
    for left in 0..64 {
        for right in 0..64 {
            let mut state = SimdState::default();
            state.xmm[0] = qwords([1 << left, 0]);
            state.xmm[1] = qwords([1 << right, 0]);
            run(op, &mut state);
            assert_eq!(state.xmm[0], (1u128 << (left + right)).to_le_bytes());
        }
    }
    let mut state = SimdState::default();
    state.xmm[0] = qwords([u64::MAX, u64::MAX]);
    run(decode(&bytes(0x3a, 0x44, 0xc0, Some(0x11))), &mut state);
    assert_eq!(state.xmm[0], qwords([0x5555_5555_5555_5555; 2]));
}

#[test]
fn crypto_invalid_shapes_are_rejected_before_any_operand_state_changes() {
    for (map, opcode, immediate) in [
        (0x38, 0xdc, None),
        (0x3a, 0xdf, Some(1)),
        (0x3a, 0x44, Some(0)),
    ] {
        let op = decode(&bytes(map, opcode, 0xc1, immediate));
        let mut malformed = Vec::new();
        let mut bad = op;
        bad.vector_bytes = 8;
        malformed.push(bad);
        bad = op;
        bad.lane_bytes = if op.lane_bytes == 1 { 8 } else { 1 };
        malformed.push(bad);
        bad = op;
        bad.source = SimdOperand::Register(SimdRegister::Mmx(0));
        malformed.push(bad);
        bad = op;
        bad.destination = SimdOperand::Register(SimdRegister::Mmx(0));
        malformed.push(bad);
        bad = op;
        bad.immediate = if immediate.is_some() { None } else { Some(0) };
        malformed.push(bad);
        bad = op;
        bad.scalar = true;
        malformed.push(bad);
        bad = op;
        bad.source = SimdOperand::Register(SimdRegister::Xmm(8));
        malformed.push(bad);
        let memory = decode(&bytes(map, opcode, 0x00, immediate));
        bad = memory;
        bad.memory_bytes = 8;
        malformed.push(bad);
        bad = memory;
        bad.aligned = false;
        malformed.push(bad);
        for bad in malformed {
            let mut state = SimdState {
                xmm: [[0xa5; 16]; 8],
                ..SimdState::default()
            };
            let original = state;
            let mut mmx = [[0x5a; 8]; 8];
            let mut memory = [0xff; 16];
            assert_eq!(
                simd::execute(bad, &mut state, &mut mmx, &mut memory, 0, 0, 0),
                Err(SimdError::InvalidDescriptor)
            );
            assert_eq!(state, original);
            assert_eq!(mmx, [[0x5a; 8]; 8]);
            assert_eq!(memory, [0xff; 16]);
        }
    }
}

#[test]
fn crypto_extended_descriptors_are_canonical_and_leave_legacy_bytes_unchanged() {
    assert_eq!(
        decode(&[0x66, 0x0f, 0xfc, 0xc1]).descriptor(),
        (0x2404, 0x0908, 0)
    );
    for (map, opcode, immediate, subcode, lane) in [
        (0x38, 0xdc, None, 0, 1),
        (0x38, 0xdd, None, 1, 1),
        (0x38, 0xde, None, 2, 1),
        (0x38, 0xdf, None, 3, 1),
        (0x38, 0xdb, None, 4, 1),
        (0x3a, 0xdf, Some(0x80), 5, 1),
        (0x3a, 0x44, Some(0xff), 6, 4),
    ] {
        let op = decode(&bytes(map, opcode, 0xc1, immediate));
        let (descriptor, operands, value) = op.descriptor();
        assert_eq!(
            descriptor,
            0x0100_2000 | subcode << 4 | lane << 10 | u32::from(immediate.is_some()) << 22
        );
        for invalid in (1..16)
            .map(|family| descriptor | family)
            .chain((25..32).map(|bit| descriptor | 1 << bit))
            .chain([descriptor | 1 << 23])
        {
            assert_eq!(
                simd::decode_descriptor(invalid, operands, value),
                Err(SimdError::InvalidDescriptor)
            );
        }
        for subcode in 7..64 {
            assert_eq!(
                simd::decode_descriptor(descriptor & !0x3f0 | subcode << 4, operands, value),
                Err(SimdError::InvalidDescriptor)
            );
        }
    }
}

#[test]
fn crypto_rejects_extra_repeat_prefixes_in_both_mandatory_prefix_orders() {
    for (map, opcode, immediate) in [
        (0x38, 0xdb, None),
        (0x38, 0xdc, None),
        (0x38, 0xdd, None),
        (0x38, 0xde, None),
        (0x38, 0xdf, None),
        (0x3a, 0xdf, Some(1)),
        (0x3a, 0x44, Some(0)),
    ] {
        for prefix in [0xf2, 0xf3] {
            for position in [0, 1] {
                let mut instruction = bytes(map, opcode, 0xc1, immediate);
                instruction.insert(position, prefix);
                let mut memory = AddressSpace::new(1).unwrap();
                memory
                    .map_zeroed(
                        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                        Permissions::ALL,
                    )
                    .unwrap();
                memory.write(GuestAddress(0x1000), &instruction).unwrap();
                assert!(
                    decode_one(&memory, GuestAddress(0x1000)).is_err(),
                    "{instruction:02x?}"
                );
            }
        }
    }
}
