use super::*;

#[test]
fn rounding_modes_apply_once_and_preserve_precision_sticky_state() {
    for (element, one, half_ulp, sign) in [
        (Element::Single, 0x3f80_0000, 0x3380_0000, 0x8000_0000),
        (
            Element::Double,
            0x3ff0_0000_0000_0000,
            0x3ca0_0000_0000_0000,
            0x8000_0000_0000_0000,
        ),
    ] {
        let shape = FmaShape::Scalar { element };
        for rounding in 0..4 {
            for negative in [false, true] {
                let mut state = seed();
                state.simd.mxcsr = 0x1f88 | (rounding << 13);
                set_lane(
                    &mut state,
                    0,
                    0,
                    element,
                    one | if negative { sign } else { 0 },
                );
                set_lane(&mut state, 1, 0, element, one);
                set_lane(
                    &mut state,
                    2,
                    0,
                    element,
                    half_ulp | if negative { sign } else { 0 },
                );
                let increment = (negative && rounding == 1) || (!negative && rounding == 2);
                let value = (one + u64::from(increment)) | if negative { sign } else { 0 };
                let mut expected = expected_result(state, shape, &[value]);
                expected.simd.mxcsr |= 0x20;
                assert_eq!(
                    state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order213, shape), &[]),
                    Ok(())
                );
                assert_eq!(state, expected);
            }
        }
    }
}

#[test]
fn daz_and_ftz_keep_exact_tiny_mask_and_signed_zero_rules() {
    for (element, minimum, half, one, sign) in [
        (
            Element::Single,
            0x0080_0000,
            0x3f00_0000,
            0x3f80_0000,
            0x8000_0000,
        ),
        (
            Element::Double,
            0x0010_0000_0000_0000,
            0x3fe0_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0x8000_0000_0000_0000,
        ),
    ] {
        let shape = FmaShape::Scalar { element };
        for (mxcsr, result, flags, trap) in [
            (0x1f80, minimum >> 1, 0, false),
            (0x9f80, 0, 0x30, false),
            (0x1780, minimum >> 1, 0x10, true),
        ] {
            for negative in [false, true] {
                let mut state = seed();
                state.simd.mxcsr = mxcsr;
                set_lane(
                    &mut state,
                    0,
                    0,
                    element,
                    minimum | if negative { sign } else { 0 },
                );
                set_lane(&mut state, 1, 0, element, 0);
                set_lane(&mut state, 2, 0, element, half);
                let mut expected = if trap {
                    state
                } else {
                    expected_result(state, shape, &[result | if negative { sign } else { 0 }])
                };
                expected.simd.mxcsr |= flags;
                let outcome = if trap {
                    Err(ArithmeticError::FloatingPoint(expected.simd.mxcsr))
                } else {
                    Ok(())
                };
                assert_eq!(
                    state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
                    outcome
                );
                assert_eq!(state, expected);
            }
        }
        for (mxcsr, value, flags) in [(0x1f80, sign | 1, 2), (0x1fc0, 0, 0)] {
            let mut state = seed();
            state.simd.mxcsr = mxcsr;
            set_lane(&mut state, 0, 0, element, sign | 1);
            set_lane(&mut state, 1, 0, element, 0);
            set_lane(&mut state, 2, 0, element, one);
            let mut expected = expected_result(state, shape, &[value]);
            expected.simd.mxcsr |= flags;
            assert_eq!(
                state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
                Ok(())
            );
            assert_eq!(state, expected);
        }
    }
}

#[test]
fn upper_lane_precomputation_fault_suppresses_lower_precision_publication() {
    let shape = FmaShape::Packed {
        width: VectorWidth::Ymm,
        element: Element::Single,
    };
    let mut state = seed();
    fill(&mut state, 0, Element::Single, 0x3f80_0000);
    fill(&mut state, 1, Element::Single, 0x3f80_0000);
    fill(&mut state, 2, Element::Single, 0);
    set_lane(&mut state, 2, 0, Element::Single, 0x3380_0000);
    set_lane(&mut state, 2, 7, Element::Single, 0x7f80_0001);
    state.simd.mxcsr = 0x1f02;
    let mut expected = state;
    expected.simd.mxcsr = 0x1f03;
    assert_eq!(
        state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order213, shape), &[]),
        Err(ArithmeticError::FloatingPoint(0x1f03))
    );
    assert_eq!(state, expected);
}

#[test]
fn unmasked_postcomputation_faults_suppress_precision_and_keep_destination() {
    let shape = FmaShape::Scalar {
        element: Element::Single,
    };
    for (mxcsr, a, b, c, expected_mxcsr) in [
        (0x1b80, 0x7f7f_ffff, 0x4000_0000, 0, 0x1b88),
        (0x1780, 0x0080_0000, 0x3f7f_ffff, 0, 0x1790),
        (0x0f80, 0x3f80_0000, 0x3f80_0000, 0x3380_0000, 0x0fa0),
    ] {
        let mut state = seed();
        state.simd.mxcsr = mxcsr;
        set_lane(&mut state, 0, 0, Element::Single, a);
        set_lane(&mut state, 1, 0, Element::Single, c);
        set_lane(&mut state, 2, 0, Element::Single, b);
        let mut expected = state;
        expected.simd.mxcsr = expected_mxcsr;
        assert_eq!(
            state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
            Err(ArithmeticError::FloatingPoint(expected_mxcsr))
        );
        assert_eq!(state, expected);
    }
}

#[test]
fn rounded_to_minimum_normal_still_reports_tiny_inexact_result() {
    let shape = FmaShape::Scalar {
        element: Element::Single,
    };
    for (mxcsr, result) in [
        (0x1f80, 0x0080_0000),
        (0x3f80, 0x007f_ffff),
        (0x5f80, 0x0080_0000),
        (0x7f80, 0x007f_ffff),
    ] {
        let mut state = seed();
        state.simd.mxcsr = mxcsr;
        set_lane(&mut state, 0, 0, Element::Single, 0x0080_0000);
        set_lane(&mut state, 1, 0, Element::Single, 0);
        set_lane(&mut state, 2, 0, Element::Single, 0x3f7f_ffff);
        let mut expected = expected_result(state, shape, &[result]);
        expected.simd.mxcsr |= 0x30;
        assert_eq!(
            state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
            Ok(())
        );
        assert_eq!(state, expected);
    }
}
