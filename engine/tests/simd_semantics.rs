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
fn unbounded_tininess_survives_rounding_to_the_smallest_normal() {
    for width in [4, 8] {
        for (mxcsr, expected_mxcsr, zero, trap) in [
            (0x1780, 0x1790, false, true),
            (0x1f80, 0x1fb0, false, false),
            (0x9f80, 0x9fb0, true, false),
        ] {
            let mut state = SimdState {
                mxcsr,
                ..SimdState::default()
            };
            let mut op = operation(K::Float(F::Multiply), width);
            op.scalar = true;
            if width == 4 {
                state.xmm[0] = dwords([0x0080_0000, 1, 2, 3]);
                state.xmm[1] = dwords([0x3f7f_ffff, 0, 0, 0]);
            } else {
                state.xmm[0][..8].copy_from_slice(&0x0010_0000_0000_0000u64.to_le_bytes());
                state.xmm[1][..8].copy_from_slice(&0x3fef_ffff_ffff_ffffu64.to_le_bytes());
                state.xmm[0][8..].fill(0xa5);
            }
            let before = state.xmm;
            if trap {
                assert_eq!(
                    run(op, &mut state),
                    Err(SimdError::FloatingPoint(expected_mxcsr))
                );
                assert_eq!(state.xmm, before);
            } else {
                run(op, &mut state).unwrap();
                let mut wanted = before[0];
                if zero {
                    wanted[..usize::from(width)].fill(0);
                }
                assert_eq!(state.xmm[0], wanted);
            }
            assert_eq!(state.mxcsr, expected_mxcsr);
        }
    }
    for (mxcsr, expected_mxcsr, zero, trap) in [
        (0x1780, 0x1790, false, true),
        (0x1f80, 0x1fb0, false, false),
        (0x9f80, 0x9fb0, true, false),
    ] {
        let mut state = SimdState {
            mxcsr,
            ..SimdState::default()
        };
        state.xmm[0] = dwords([1, 2, 3, 4]);
        state.xmm[1][..8].copy_from_slice(&0x380f_ffff_e000_0000u64.to_le_bytes());
        let before = state.xmm;
        let mut op = operation(
            K::Convert {
                kind: C::DoubleToSingle,
                truncate: false,
            },
            4,
        );
        op.scalar = true;
        if trap {
            assert_eq!(
                run(op, &mut state),
                Err(SimdError::FloatingPoint(expected_mxcsr))
            );
            assert_eq!(state.xmm, before);
        } else {
            run(op, &mut state).unwrap();
            assert_eq!(
                state.xmm[0],
                dwords([if zero { 0 } else { 0x0080_0000 }, 2, 3, 4])
            );
        }
        assert_eq!(state.mxcsr, expected_mxcsr);
    }
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
    let mut x87 = X87State {
        status: 3 << 11,
        tag: 0xffff,
        ..X87State::default()
    };
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
fn descriptor_errors_precede_invalid_mxcsr_without_arena_mutation() {
    use ring3_engine::abi::{
        arena::{ARENA_SIZE, SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_MXCSR_OFFSET, SIMD_SIZE, X87_SIZE, encode_simd, encode_x87},
    };
    let mut arena = vec![0; ARENA_SIZE];
    encode_x87(
        &X87State::default(),
        &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )
    .unwrap();
    encode_simd(
        &SimdState {
            xmm: std::array::from_fn(|index| [index as u8 + 0xa0; 16]),
            ..SimdState::default()
        },
        &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )
    .unwrap();
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0x5a);
    let mxcsr = SIMD_OFFSET + SIMD_MXCSR_OFFSET;
    arena[mxcsr..mxcsr + 4].copy_from_slice(&0x0001_1f80u32.to_le_bytes());
    let before = arena.clone();
    assert_eq!(
        operation(K::Packed(P::Add), 1).descriptor(),
        (0x2404, 0x0908, 0)
    );
    for (descriptor, operands, immediate) in [
        (0x8000_2404, 0x0908, 0),
        (0x2404, 0x093f, 0),
        (0x2404, 0x0908, 256),
        (0x2404, 0x0908, 1),
        (0x3c04, 0x0908, 0),
        (0x3404, 0x0908, 0),
    ] {
        assert_eq!(
            simd::execute_descriptor(&mut arena, descriptor, operands, immediate, 0, 0, 0xcd7),
            Err(SimdError::InvalidDescriptor)
        );
        assert_eq!(arena, before);
    }
    assert_eq!(
        simd::execute_descriptor(&mut arena, 0x2404, 0x0908, 0, 0, 0, 0xcd7),
        Err(SimdError::InvalidState)
    );
    assert_eq!(arena, before);
}

#[test]
fn typed_operation_errors_precede_invalid_mxcsr_without_output_mutation() {
    let mut state = SimdState {
        mxcsr: 0x0001_1f80,
        xmm: [[0xa5; 16]; 8],
    };
    let mut mmx = [[0x3c; 8]; 8];
    let mut memory = [0x5a; 16];
    let before = (state, mmx, memory);
    let valid = operation(K::Packed(P::Add), 1);
    let invalid = SimdOperation {
        source: O::Register(R::Xmm(8)),
        ..valid
    };
    for (op, error) in [
        (invalid, SimdError::InvalidDescriptor),
        (valid, SimdError::InvalidState),
    ] {
        assert_eq!(
            simd::execute(op, &mut state, &mut mmx, &mut memory, 0, 0, 0xcd7),
            Err(error)
        );
        assert_eq!((state, mmx, memory), before);
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

#[test]
fn normal_power_of_two_products_preserve_literal_significands_and_scalar_upper_lanes() {
    for (width, a, b, expected) in [
        (4, 0x0080_0000u64, 0x3f80_0000u64, 0x0080_0000u64),
        (4, 0x7f7f_ffff, 0x3f80_0000, 0x7f7f_ffff),
        (4, 0x8100_0001, 0xbf00_0000, 0x0080_0001),
        (4, 0x7eff_ffff, 0x4000_0000, 0x7f7f_ffff),
        (4, 0x3f80_0001, 0xc000_0000, 0xc000_0001),
        (
            8,
            0x0010_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0x0010_0000_0000_0000,
        ),
        (
            8,
            0x7fef_ffff_ffff_ffff,
            0x3ff0_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
        ),
        (
            8,
            0x8020_0000_0000_0001,
            0xbfe0_0000_0000_0000,
            0x0010_0000_0000_0001,
        ),
        (
            8,
            0x7fdf_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0x7fef_ffff_ffff_ffff,
        ),
        (
            8,
            0x3ff0_0000_0000_0001,
            0xc000_0000_0000_0000,
            0xc000_0000_0000_0001,
        ),
    ] {
        for (left, right) in [(a, b), (b, a)] {
            let mut state = SimdState {
                xmm: std::array::from_fn(|index| [0xa0 + index as u8; 16]),
                ..SimdState::default()
            };
            let length = usize::from(width);
            state.xmm[0][..length].copy_from_slice(&left.to_le_bytes()[..length]);
            state.xmm[1][..length].copy_from_slice(&right.to_le_bytes()[..length]);
            let mut wanted = state;
            wanted.xmm[0][..length].copy_from_slice(&expected.to_le_bytes()[..length]);
            let mut op = operation(K::Float(F::Multiply), width);
            op.scalar = true;
            assert_eq!(run(op, &mut state).unwrap().flags, 0xcd7);
            assert_eq!(state, wanted);
        }
    }
}

#[test]
fn exact_packed_products_ignore_rounding_masks_daz_ftz_and_keep_sticky_status() {
    let qwords = |values: [u64; 2]| {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&values[0].to_le_bytes());
        bytes[8..].copy_from_slice(&values[1].to_le_bytes());
        bytes
    };
    for (width, left, right, expected) in [
        (
            4,
            dwords([0x0080_0000, 0x7f7f_ffff, 0x8100_0001, 0x3f80_0001]),
            dwords([0x3f80_0000, 0x3f80_0000, 0xbf00_0000, 0xc000_0000]),
            dwords([0x0080_0000, 0x7f7f_ffff, 0x0080_0001, 0xc000_0001]),
        ),
        (
            8,
            qwords([0x8020_0000_0000_0001, 0x7fdf_ffff_ffff_ffff]),
            qwords([0xbfe0_0000_0000_0000, 0x4000_0000_0000_0000]),
            qwords([0x0010_0000_0000_0001, 0x7fef_ffff_ffff_ffff]),
        ),
    ] {
        for round in 0..4 {
            for control in [0, 0x1f80, 0x0040, 0x8000, 0x9fff] {
                let mut state = SimdState {
                    mxcsr: control | (round << 13),
                    xmm: [[0xa5; 16]; 8],
                };
                state.xmm[0] = left;
                state.xmm[1] = right;
                let mut wanted = state;
                wanted.xmm[0] = expected;
                let mut mmx = [[0x3c; 8]; 8];
                let mut memory = [0x5a; 16];
                let result = simd::execute(
                    operation(K::Float(F::Multiply), width),
                    &mut state,
                    &mut mmx,
                    &mut memory,
                    0,
                    0,
                    0xcd7,
                )
                .unwrap();
                assert_eq!(result.flags, 0xcd7);
                assert_eq!(state, wanted);
                assert_eq!(mmx, [[0x3c; 8]; 8]);
                assert_eq!(memory, [0x5a; 16]);
            }
        }
    }
}

#[test]
fn product_guard_neighbors_keep_literal_fallback_results_and_packed_exception_atomicity() {
    for (width, a, b, mxcsr, expected, status) in [
        (4, 0x8000_0000u64, 0xc000_0000u64, 0x1f80, 0u64, 0),
        (4, 0x007f_ffff, 0x4000_0000, 0x1f80, 0x00ff_fffe, 2),
        (4, 0x007f_ffff, 0x4000_0000, 0x1fc0, 0, 0),
        (4, 0x7f80_0000, 0x4000_0000, 0x1f80, 0x7f80_0000, 0),
        (4, 0x7fc0_1234, 0x4000_0000, 0x1f80, 0x7fc0_1234, 0),
        (4, 0x7f80_1234, 0x4000_0000, 0x1f80, 0x7fc0_1234, 1),
        (4, 0x0080_0000, 0x3f00_0000, 0x1f80, 0x0040_0000, 0),
        (4, 0x7f7f_ffff, 0x4000_0000, 0x1f80, 0x7f80_0000, 0x28),
        (4, 0x3f80_0001, 0x3f80_0001, 0x1f80, 0x3f80_0002, 0x20),
        (4, 0x3f80_0001, 0x3f80_0001, 0x5f80, 0x3f80_0003, 0x20),
        (
            8,
            0x8000_0000_0000_0000,
            0xc000_0000_0000_0000,
            0x1f80,
            0,
            0,
        ),
        (
            8,
            0x000f_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0x1f80,
            0x001f_ffff_ffff_fffe,
            2,
        ),
        (
            8,
            0x000f_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0x1fc0,
            0,
            0,
        ),
        (
            8,
            0x7ff0_0000_0000_0000,
            0x4000_0000_0000_0000,
            0x1f80,
            0x7ff0_0000_0000_0000,
            0,
        ),
        (
            8,
            0x7ff8_0000_0000_1234,
            0x4000_0000_0000_0000,
            0x1f80,
            0x7ff8_0000_0000_1234,
            0,
        ),
        (
            8,
            0x7ff0_0000_0000_1234,
            0x4000_0000_0000_0000,
            0x1f80,
            0x7ff8_0000_0000_1234,
            1,
        ),
        (
            8,
            0x0010_0000_0000_0000,
            0x3fe0_0000_0000_0000,
            0x1f80,
            0x0008_0000_0000_0000,
            0,
        ),
        (
            8,
            0x7fef_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0x1f80,
            0x7ff0_0000_0000_0000,
            0x28,
        ),
        (
            8,
            0x3ff0_0000_0000_0001,
            0x3ff0_0000_0000_0001,
            0x1f80,
            0x3ff0_0000_0000_0002,
            0x20,
        ),
        (
            8,
            0x3ff0_0000_0000_0001,
            0x3ff0_0000_0000_0001,
            0x5f80,
            0x3ff0_0000_0000_0003,
            0x20,
        ),
    ] {
        let mut state = SimdState {
            mxcsr,
            xmm: [[0xa5; 16]; 8],
        };
        let length = usize::from(width);
        state.xmm[0][..length].copy_from_slice(&a.to_le_bytes()[..length]);
        state.xmm[1][..length].copy_from_slice(&b.to_le_bytes()[..length]);
        let mut wanted = state;
        wanted.xmm[0][..length].copy_from_slice(&expected.to_le_bytes()[..length]);
        wanted.mxcsr |= status;
        let mut op = operation(K::Float(F::Multiply), width);
        op.scalar = true;
        assert_eq!(run(op, &mut state).unwrap().flags, 0xcd7);
        assert_eq!(state, wanted);
    }
    for width in [4, 8] {
        let mut state = SimdState {
            mxcsr: 0x1b80,
            xmm: [[0xa5; 16]; 8],
        };
        if width == 4 {
            state.xmm[0] = dwords([0x3f80_0001, 0x7f7f_ffff, 0x3f80_0000, 0xbf80_0000]);
            state.xmm[1] = dwords([0x4000_0000; 4]);
        } else {
            state.xmm[0][..8].copy_from_slice(&0x3ff0_0000_0000_0001u64.to_le_bytes());
            state.xmm[0][8..].copy_from_slice(&0x7fef_ffff_ffff_ffffu64.to_le_bytes());
            state.xmm[1][..8].copy_from_slice(&0x4000_0000_0000_0000u64.to_le_bytes());
            state.xmm[1][8..].copy_from_slice(&0x4000_0000_0000_0000u64.to_le_bytes());
        }
        let before = state.xmm;
        assert_eq!(
            run(operation(K::Float(F::Multiply), width), &mut state),
            Err(SimdError::FloatingPoint(0x1b88))
        );
        assert_eq!(state.xmm, before);
        assert_eq!(state.mxcsr, 0x1b88);
    }
}
