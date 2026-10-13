use super::*;

#[test]
fn lane_memory_geometry_is_full_thirty_two_bytes_and_refusals_are_atomic() {
    for (kind, lane) in [
        (ShuffleKind::UnpackLow, 1),
        (ShuffleKind::PackUnsigned, 2),
        (ShuffleKind::Dwords, 4),
        (ShuffleKind::AlignRight, 1),
        (ShuffleKind::Bytes, 1),
        (ShuffleKind::BlendVariable, 1),
    ] {
        let operation = RearrangeOperation::new(kind, lane, 0).unwrap();
        let narrow = RearrangeInstruction::new(
            operation,
            VectorRegister(0),
            (!operation.unary()).then_some(VectorRegister(0)),
            MoveSource::Memory,
            operation.uses_mask().then_some(VectorRegister(1)),
        )
        .unwrap();
        let wide = narrow.with_width(VectorWidth::Ymm).unwrap();
        assert_eq!(narrow.source_bytes(), 16);
        assert_eq!(wide.source_bytes(), 32);
        assert_eq!(
            wide.memory(),
            Some(MemoryRequirement {
                access: Access::Read,
                bytes: 32,
                alignment: 1
            })
        );
        let mut state = VectorState::default();
        state.simd.xmm[0] = [0xa7; 16];
        state.upper[0] = [0x3d; 16];
        for bytes in [0, 16, 31, 33] {
            let before = state;
            assert_eq!(
                state.execute_rearrange(wide, &vec![0; bytes]),
                Err(RearrangeError::OperandLength)
            );
            assert_eq!(state, before);
        }
        state.simd.mxcsr = 0x1_0000;
        let before = state;
        assert_eq!(
            state.execute_rearrange(wide, &[0; 32]),
            Err(RearrangeError::InvalidState)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn aliased_explicit_masks_and_sources_are_captured_before_both_halves_publish() {
    let operation = RearrangeOperation::new(ShuffleKind::BlendVariable, 1, 0).unwrap();
    for destination in 0..8 {
        for first in 0..8 {
            for second in [first, destination, (first + 1) % 8] {
                for mask in [destination, first, second] {
                    let mut state = VectorState::default();
                    state.simd.mxcsr = 0xffff;
                    state.simd.xmm =
                        std::array::from_fn(|r| std::array::from_fn(|b| (r * 37 + b * 13) as u8));
                    state.upper = std::array::from_fn(|r| {
                        std::array::from_fn(|b| (r * 23 + b * 19 + 0x80) as u8)
                    });
                    let before = state;
                    let instruction = RearrangeInstruction::new(
                        operation,
                        VectorRegister(destination),
                        Some(VectorRegister(first)),
                        MoveSource::Register(VectorRegister(second)),
                        Some(VectorRegister(mask)),
                    )
                    .unwrap()
                    .with_width(VectorWidth::Ymm)
                    .unwrap();
                    let a = before.read(VectorRegister(first));
                    let b = before.read(VectorRegister(second));
                    let m = before.read(VectorRegister(mask));
                    let value: [u8; 32] = std::array::from_fn(|byte| {
                        if m[byte] & 0x80 != 0 {
                            b[byte]
                        } else {
                            a[byte]
                        }
                    });
                    state.execute_rearrange(instruction, &[]).unwrap();
                    let mut expected = before;
                    expected.simd.xmm[usize::from(destination)].copy_from_slice(&value[..16]);
                    expected.upper[usize::from(destination)].copy_from_slice(&value[16..]);
                    assert_eq!(state, expected);
                }
            }
        }
    }
}

#[test]
fn lane_upper_sources_are_distinct_from_the_preserved_low_xmm_widening_windows() {
    let mut state = VectorState::default();
    state.simd.xmm[0] = std::array::from_fn(|b| b as u8);
    state.upper[0] = std::array::from_fn(|b| b as u8 + 64);
    let shuffle = RearrangeInstruction::new(
        RearrangeOperation::new(ShuffleKind::Dwords, 4, 0x1b).unwrap(),
        VectorRegister(0),
        None,
        MoveSource::Register(VectorRegister(0)),
        None,
    )
    .unwrap()
    .with_width(VectorWidth::Ymm)
    .unwrap();
    state.execute_rearrange(shuffle, &[]).unwrap();
    assert_eq!(
        state.simd.xmm[0],
        [12, 13, 14, 15, 8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3]
    );
    assert_eq!(
        state.upper[0],
        [
            76, 77, 78, 79, 72, 73, 74, 75, 68, 69, 70, 71, 64, 65, 66, 67
        ]
    );
    let widen = RearrangeInstruction::new(
        RearrangeOperation::new(ShuffleKind::SignExtendBytes, 8, 0).unwrap(),
        VectorRegister(0),
        None,
        MoveSource::Register(VectorRegister(0)),
        None,
    )
    .unwrap()
    .with_width(VectorWidth::Ymm)
    .unwrap();
    state.simd.xmm[0][..4].copy_from_slice(&[0x80, 0x7f, 0xff, 1]);
    let before = state;
    for upper in [[0x35; 16], [0xca; 16]] {
        state = before;
        state.upper[0] = upper;
        state.execute_rearrange(widen, &[]).unwrap();
        let value = state.read(VectorRegister(0));
        for (lane, number) in [-128_i64, 127, -1, 1].into_iter().enumerate() {
            assert_eq!(&value[lane * 8..lane * 8 + 8], &number.to_le_bytes());
        }
    }
}
