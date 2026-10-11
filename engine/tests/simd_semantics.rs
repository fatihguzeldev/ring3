use ring3_engine::cpu::x86::{
    X87State,
    simd::{
        self, ConvertKind as C, FloatKind as F, PackedKind as P, ShuffleKind as S, SimdError,
        SimdKind as K, SimdOperand as O, SimdOperation, SimdRegister as R, SimdState,
    },
};

fn operation(kind: K, width: u8) -> SimdOperation {
    SimdOperation {
        kind,
        destination: O::Register(R::Xmm(0)),
        source: O::Register(R::Xmm(1)),
        vector_bytes: 16,
        lane_bytes: width,
        memory_bytes: 0,
        immediate: None,
        aligned: false,
        scalar: false,
        implicit_address: None,
    }
}
fn run(op: SimdOperation, state: &mut SimdState) -> Result<simd::SimdResult, SimdError> {
    simd::execute(op, state, &mut [[0; 8]; 8], &mut [0; 16], 0, 0, 0xcd7)
}
fn dwords(values: [u32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (i, v) in values.into_iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn packed_integer_saturation_multiply_and_full_count_are_exact() {
    let mut state = SimdState::default();
    state.xmm[0] = [0x7f; 16];
    state.xmm[1] = [1; 16];
    run(operation(K::Packed(P::AddSignedSaturate), 1), &mut state).unwrap();
    assert_eq!(state.xmm[0], [0x7f; 16]);
    state.xmm[0] = [0; 16];
    state.xmm[1] = [1; 16];
    run(
        operation(K::Packed(P::SubtractUnsignedSaturate), 1),
        &mut state,
    )
    .unwrap();
    assert_eq!(state.xmm[0], [0; 16]);
    state.xmm[0] = [
        0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80, 0, 0x80,
    ];
    state.xmm[1] = state.xmm[0];
    run(operation(K::Packed(P::MultiplyAddWords), 2), &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([0x8000_0000; 4]));
    state.xmm[0] = [255; 16];
    state.xmm[1] = [0; 16];
    state.xmm[1][4] = 1;
    run(operation(K::Packed(P::ShiftRight), 8), &mut state).unwrap();
    assert_eq!(state.xmm[0], [0; 16]);
    state.xmm[0] = dwords([u32::MAX, 7, 0x8000_0000, 9]);
    state.xmm[1] = dwords([u32::MAX, 11, 2, 13]);
    run(
        operation(K::Packed(P::MultiplyUnsignedDword), 4),
        &mut state,
    )
    .unwrap();
    assert_eq!(
        u64::from_le_bytes(state.xmm[0][..8].try_into().unwrap()),
        0xffff_fffe_0000_0001
    );
    assert_eq!(
        u64::from_le_bytes(state.xmm[0][8..].try_into().unwrap()),
        0x1_0000_0000
    );
}

#[test]
fn shuffles_capture_original_lanes_and_masks() {
    let mut state = SimdState::default();
    state.xmm[0] = dwords([1, 2, 3, 4]);
    let mut op = operation(K::Shuffle(S::Dwords), 4);
    op.source = op.destination;
    op.immediate = Some(0x1b);
    run(op, &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([4, 3, 2, 1]));
    state.xmm[0] = dwords([1, 2, 3, 4]);
    state.xmm[1] = dwords([5, 6, 7, 8]);
    op = operation(K::Shuffle(S::Singles), 4);
    op.immediate = Some(0x4e);
    run(op, &mut state).unwrap();
    assert_eq!(state.xmm[0], dwords([3, 4, 5, 6]));
    state.xmm[1] = [0x80; 16];
    op = operation(K::Shuffle(S::MaskBytes), 1);
    op.destination = O::Register(R::Gpr(ring3_engine::cpu::x86::Register32::Eax));
    assert_eq!(run(op, &mut state).unwrap().gpr, 0xffff);
}

#[test]
fn floating_rounding_nan_priority_and_unmasked_transaction() {
    let mut state = SimdState::default();
    state.xmm[0] = dwords([0x3f80_0000, 0, 0, 0]);
    state.xmm[1] = dwords([0x3380_0000, 0, 0, 0]);
    let mut op = operation(K::Float(F::Add), 4);
    op.scalar = true;
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0x3f80_0000);
    assert_eq!(state.mxcsr & 32, 32);
    state.mxcsr = 0x5f80;
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0x3f80_0001);
    state.mxcsr = 0x1f80;
    state.xmm[0] = dwords([0x7f80_1234, 1, 2, 3]);
    state.xmm[1] = dwords([0x7fc0_5678, 4, 5, 6]);
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0x7fc0_1234);
    assert_eq!(state.mxcsr & 1, 1);
    state.mxcsr = 0x1d80;
    state.xmm[0] = dwords([0x3f80_0000, 1, 2, 3]);
    state.xmm[1] = [0; 16];
    let before = state.xmm;
    op.kind = K::Float(F::Divide);
    assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(0x1d84)));
    assert_eq!(state.xmm, before);
}

#[test]
fn unmasked_exception_priority_suppresses_lower_status_without_destination_commit() {
    let mut op = operation(K::Float(F::Multiply), 4);
    op.scalar = true;
    for (mxcsr, a, b, expected) in [
        (0x1b80, 0x7f7f_ffff, 0x4000_0000, 0x1b88),
        (0x1780, 0x0080_0000, 0x3dcc_cccd, 0x1790),
        (0x1780, 0x0080_0000, 0x3f00_0000, 0x1790),
        (0x9780, 0x0080_0000, 0x3f00_0000, 0x9790),
        (0x1e80, 1, 0x3f00_0000, 0x1e82),
    ] {
        let mut state = SimdState {
            mxcsr,
            ..SimdState::default()
        };
        state.xmm[0] = dwords([a, 1, 2, 3]);
        state.xmm[1] = dwords([b, 4, 5, 6]);
        let before = state.xmm;
        assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(expected)));
        assert_eq!(state.xmm, before);
        assert_eq!(state.mxcsr, expected);
    }
    let mut state = SimdState {
        mxcsr: 0x1f00,
        ..SimdState::default()
    };
    state.xmm[0] = dwords([0x7f80_0001, 0x7f7f_ffff, 0, 0]);
    state.xmm[1] = dwords([0x3f80_0000, 0x4000_0000, 0, 0]);
    op.scalar = false;
    let before = state.xmm;
    assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(0x1f01)));
    assert_eq!(state.xmm, before);
    state.mxcsr = 0x1b80;
    state.xmm[0] = dwords([0x7f7f_ffff, 0x3f80_0001, 0, 0]);
    state.xmm[1] = dwords([0x4000_0000, 0x3f80_0001, 0, 0]);
    let before = state.xmm;
    assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(0x1ba8)));
    assert_eq!(state.xmm, before);
    state.mxcsr = 0x1f00;
    state.xmm[0] = dwords([0x7f80_0001, 1, 0, 0]);
    state.xmm[1] = dwords([0x3f80_0000, 0x3f80_0000, 0, 0]);
    assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(0x1f03)));
}

#[test]
fn daz_ftz_minimum_and_compare_flags_follow_mxcsr() {
    let mut state = SimdState {
        mxcsr: 0x1fc0,
        ..SimdState::default()
    };
    state.xmm[0] = dwords([1, 0, 0, 0]);
    state.xmm[1] = [0; 16];
    let mut op = operation(K::Float(F::Add), 4);
    op.scalar = true;
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0);
    assert_eq!(state.mxcsr & 63, 0);
    state.mxcsr = 0x9f80;
    state.xmm[0] = dwords([0x0080_0000, 0, 0, 0]);
    state.xmm[1] = dwords([0x3f00_0000, 0, 0, 0]);
    op.kind = K::Float(F::Multiply);
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0);
    assert_eq!(state.mxcsr & 63, 0x30);
    state.mxcsr = 0x1f80;
    state.xmm[0] = dwords([0x8000_0000, 0, 0, 0]);
    state.xmm[1] = [0; 16];
    op.kind = K::Float(F::Minimum);
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0);
    state.xmm[0] = dwords([0x7fc0_0000, 0, 0, 0]);
    op.kind = K::Float(F::CompareFlagsQuiet);
    let result = run(op, &mut state).unwrap();
    assert_eq!(result.flags, 0x447);
    assert_eq!(state.mxcsr & 1, 0);
    op.kind = K::Float(F::Minimum);
    run(op, &mut state).unwrap();
    assert_eq!(word(&state.xmm[0], 0), 0);
    assert_eq!(state.mxcsr & 1, 1);
    state.mxcsr = 0x1f00;
    state.xmm[0] = dwords([0x7fc0_1234, 1, 2, 3]);
    state.xmm[1] = dwords([0x3f80_0000, 4, 5, 6]);
    let before = state.xmm;
    assert_eq!(run(op, &mut state), Err(SimdError::FloatingPoint(0x1f01)));
    assert_eq!(state.xmm, before);
}

#[test]
fn conversion_invalid_indefinite_and_scalar_upper_preservation() {
    let mut state = SimdState::default();
    state.xmm[0] = dwords([1, 2, 3, 4]);
    state.xmm[1] = dwords([0x7fc0_1234, 0x3fc0_0000, 0xbfc0_0000, 0x4f00_0000]);
    run(
        operation(
            K::Convert {
                kind: C::SinglesToSignedDwords,
                truncate: true,
            },
            4,
        ),
        &mut state,
    )
    .unwrap();
    assert_eq!(
        state.xmm[0],
        dwords([0x8000_0000, 1, u32::MAX, 0x8000_0000])
    );
    assert_eq!(state.mxcsr & 0x21, 0x21);
    state.xmm[0] = dwords([1, 2, 3, 4]);
    state.xmm[1] = dwords([0x3f80_0000, 0, 0, 0]);
    let mut op = operation(
        K::Convert {
            kind: C::SingleToDouble,
            truncate: false,
        },
        8,
    );
    op.scalar = true;
    run(op, &mut state).unwrap();
    assert_eq!(&state.xmm[0][8..], &dwords([1, 2, 3, 4])[8..]);
    assert_eq!(
        u64::from_le_bytes(state.xmm[0][..8].try_into().unwrap()),
        0x3ff0_0000_0000_0000
    );
}

#[test]
fn fxsave_preserves_software_tail_and_restores_physical_tags() {
    let mut x87 = X87State::default();
    x87.status = 3 << 11;
    x87.tag = 0xffff;
    for index in 0..8 {
        x87.registers[index][..8]
            .copy_from_slice(&(0x8000_0000_0000_0000u64 + index as u64).to_le_bytes());
        x87.registers[index][8..].copy_from_slice(&0x3fffu16.to_le_bytes());
    }
    x87.tag &= !(3 << 6);
    x87.tag = (x87.tag & !(3 << 8)) | (1 << 8);
    x87.registers[4] = [0; 10];
    let state = SimdState {
        mxcsr: 0x5f80,
        xmm: std::array::from_fn(|i| [i as u8; 16]),
    };
    let mut image = [0xa5; 512];
    simd::save_extended(&x87, &state, &mut image);
    assert_eq!(&image[464..], &[0xa5; 48]);
    assert_eq!(&image[32..42], &x87.registers[3]);
    assert_eq!(simd::restore_extended(&image), Ok((x87, state)));
    image[26] = 1;
    assert_eq!(
        simd::restore_extended(&image),
        Err(SimdError::GeneralProtection(0))
    );
}

#[test]
fn descriptors_reject_bad_width_and_operands_before_state_mutation() {
    let op = operation(K::Packed(P::Add), 1);
    let (d, o, i) = op.descriptor();
    assert_eq!(simd::decode_descriptor(d, o, i), Ok(op));
    assert_eq!(
        simd::decode_descriptor(d | 0x8000_0000, o, i),
        Err(SimdError::InvalidDescriptor)
    );
    assert_eq!(
        simd::decode_descriptor(d, o | 63, i),
        Err(SimdError::InvalidDescriptor)
    );
    assert_eq!(
        simd::decode_descriptor(d, o, 256),
        Err(SimdError::InvalidDescriptor)
    );
    assert_eq!(
        simd::decode_descriptor(d, o, 1),
        Err(SimdError::InvalidDescriptor)
    );
    for bad in [
        SimdOperation {
            lane_bytes: 8,
            ..operation(K::Packed(P::Equal), 1)
        },
        SimdOperation {
            source: O::Register(R::Mmx(1)),
            ..operation(K::Packed(P::Add), 1)
        },
        SimdOperation {
            lane_bytes: 2,
            ..operation(K::Float(F::Add), 4)
        },
        SimdOperation {
            immediate: Some(1),
            ..operation(K::Float(F::Add), 4)
        },
        SimdOperation {
            source: O::Register(R::Xmm(8)),
            ..operation(K::Packed(P::Add), 1)
        },
        SimdOperation {
            scalar: false,
            ..operation(
                K::Convert {
                    kind: C::SingleToDouble,
                    truncate: false,
                },
                8,
            )
        },
    ] {
        let mut state = SimdState::default();
        state.xmm[0] = [0xa5; 16];
        let before = state;
        assert_eq!(run(bad, &mut state), Err(SimdError::InvalidDescriptor));
        assert_eq!(state, before);
    }
}

#[test]
fn pending_x87_fault_precedes_mmx_but_not_sse_publication() {
    use ring3_engine::abi::{
        arena::{ARENA_SIZE, SIMD_OFFSET, X87_OFFSET},
        x86::{SIMD_SIZE, X87_SIZE, X87_STATUS_OFFSET, encode_simd, encode_x87},
    };
    let mut arena = vec![0; ARENA_SIZE];
    let mut x87 = X87State::default();
    x87.control &= !1;
    x87.status = 1;
    encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    encode_simd(
        &SimdState::default(),
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    let op = SimdOperation {
        destination: O::Register(R::Mmx(0)),
        source: O::Register(R::Mmx(1)),
        vector_bytes: 8,
        ..operation(K::Packed(P::Add), 1)
    };
    let (d, o, i) = op.descriptor();
    let mut expected = arena.clone();
    expected[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
        .copy_from_slice(&0x8081u16.to_le_bytes());
    assert_eq!(
        simd::execute_descriptor(&mut arena, d, o, i, 0, 0, 0xcd7),
        Err(SimdError::X87FloatingPoint(0x8081))
    );
    assert_eq!(arena, expected);
    let (d, o, i) = operation(K::Packed(P::Add), 1).descriptor();
    assert!(simd::execute_descriptor(&mut arena, d, o, i, 0, 0, 0xcd7).is_ok());
}
