use super::*;

#[test]
fn encoded_operand_orders_keep_literal_results_for_every_shape() {
    for (element, two, three, five, results) in [
        (
            Element::Single,
            0x4000_0000,
            0x4040_0000,
            0x40a0_0000,
            [0x4150_0000, 0x4130_0000, 0x4188_0000],
        ),
        (
            Element::Double,
            0x4000_0000_0000_0000,
            0x4008_0000_0000_0000,
            0x4014_0000_0000_0000,
            [
                0x402a_0000_0000_0000,
                0x4026_0000_0000_0000,
                0x4031_0000_0000_0000,
            ],
        ),
    ] {
        for shape in shapes(element) {
            for (order, result) in [FmaOrder::Order132, FmaOrder::Order213, FmaOrder::Order231]
                .into_iter()
                .zip(results)
            {
                let mut state = seed();
                fill(&mut state, 0, element, two);
                fill(&mut state, 1, element, three);
                fill(&mut state, 2, element, five);
                let expected = expected_result(state, shape, &[result]);
                assert_eq!(
                    state.execute_fma(instruction(FmaKind::Add, order, shape), &[]),
                    Ok(())
                );
                assert_eq!(state, expected);
            }
        }
    }
}

#[test]
fn negated_and_alternating_operations_keep_even_odd_signs() {
    for (element, two, three, five, eleven, one, sign) in [
        (
            Element::Single,
            0x4000_0000,
            0x4040_0000,
            0x40a0_0000,
            0x4130_0000,
            0x3f80_0000,
            0x8000_0000,
        ),
        (
            Element::Double,
            0x4000_0000_0000_0000,
            0x4008_0000_0000_0000,
            0x4014_0000_0000_0000,
            0x4026_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0x8000_0000_0000_0000,
        ),
    ] {
        for shape in shapes(element) {
            for (kind, values) in [
                (FmaKind::Add, [eleven, eleven]),
                (FmaKind::Subtract, [one, one]),
                (FmaKind::NegativeAdd, [sign | one, sign | one]),
                (FmaKind::NegativeSubtract, [sign | eleven, sign | eleven]),
                (FmaKind::AddSubtract, [one, eleven]),
                (FmaKind::SubtractAdd, [eleven, one]),
            ] {
                if matches!(shape, FmaShape::Scalar { .. }) && kind.alternating() {
                    continue;
                }
                let mut state = seed();
                fill(&mut state, 0, element, two);
                fill(&mut state, 1, element, three);
                fill(&mut state, 2, element, five);
                let expected = expected_result(state, shape, &values);
                assert_eq!(
                    state.execute_fma(instruction(kind, FmaOrder::Order213, shape), &[]),
                    Ok(())
                );
                assert_eq!(state, expected);
            }
        }
    }
}

#[test]
fn cancellation_retains_the_fused_product_in_both_formats() {
    for (element, a, b, c, result) in [
        (
            Element::Single,
            0x3f80_0001,
            0x3f7f_fffe,
            0xbf80_0000,
            0xa880_0000,
        ),
        (
            Element::Double,
            0x3ff0_0000_0000_0001,
            0x3fef_ffff_ffff_fffe,
            0xbff0_0000_0000_0000,
            0xb970_0000_0000_0000,
        ),
    ] {
        for shape in shapes(element) {
            let mut state = seed();
            fill(&mut state, 0, element, a);
            fill(&mut state, 1, element, c);
            fill(&mut state, 2, element, b);
            let expected = expected_result(state, shape, &[result]);
            assert_eq!(
                state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
                Ok(())
            );
            assert_eq!(state, expected);
        }
    }
}

#[test]
fn nan_selection_precedes_sign_changes_and_checks_all_signaling_inputs() {
    let shape = FmaShape::Scalar {
        element: Element::Single,
    };
    for kind in [
        FmaKind::Add,
        FmaKind::Subtract,
        FmaKind::NegativeAdd,
        FmaKind::NegativeSubtract,
    ] {
        for (order, value) in [
            (FmaOrder::Order132, 0xffc0_0011),
            (FmaOrder::Order213, 0x7fc0_0022),
            (FmaOrder::Order231, 0x7fc0_0022),
        ] {
            let mut state = seed();
            set_lane(&mut state, 0, 0, Element::Single, 0xffc0_0011);
            set_lane(&mut state, 1, 0, Element::Single, 0x7fc0_0022);
            set_lane(&mut state, 2, 0, Element::Single, 0x7f80_0033);
            let mut expected = expected_result(state, shape, &[value]);
            expected.simd.mxcsr |= 1;
            assert_eq!(
                state.execute_fma(instruction(kind, order, shape), &[]),
                Ok(())
            );
            assert_eq!(state, expected);
        }
    }
    let mut state = seed();
    set_lane(&mut state, 0, 0, Element::Single, 0);
    set_lane(&mut state, 1, 0, Element::Single, 0x7fc0_0044);
    set_lane(&mut state, 2, 0, Element::Single, 0x7f80_0000);
    let expected = expected_result(state, shape, &[0x7fc0_0044]);
    assert_eq!(
        state.execute_fma(instruction(FmaKind::Add, FmaOrder::Order132, shape), &[]),
        Ok(())
    );
    assert_eq!(state, expected);
}

#[test]
fn zero_signs_follow_the_primary_table_in_all_rounding_modes() {
    let shape = FmaShape::Scalar {
        element: Element::Single,
    };
    let kinds = [
        FmaKind::Add,
        FmaKind::Subtract,
        FmaKind::NegativeAdd,
        FmaKind::NegativeSubtract,
    ];
    // -1 selects negative zero only when rounding down; 0 and 1 are fixed signs.
    for (product_sign, addend_sign, signs) in [
        (0, 0, [0, -1, -1, 1]),
        (0, 0x8000_0000, [-1, 0, 1, -1]),
        (0x8000_0000, 0, [-1, 1, 0, -1]),
        (0x8000_0000, 0x8000_0000, [1, -1, -1, 0]),
    ] {
        for rounding in 0..4 {
            for (kind, sign) in kinds.into_iter().zip(signs) {
                let mut state = seed();
                state.simd.mxcsr |= rounding << 13;
                set_lane(&mut state, 0, 0, Element::Single, product_sign);
                set_lane(&mut state, 1, 0, Element::Single, 0x3f80_0000);
                set_lane(&mut state, 2, 0, Element::Single, addend_sign);
                let value = if sign == 1 || sign == -1 && rounding == 1 {
                    0x8000_0000
                } else {
                    0
                };
                let expected = expected_result(state, shape, &[value]);
                assert_eq!(
                    state.execute_fma(instruction(kind, FmaOrder::Order213, shape), &[]),
                    Ok(())
                );
                assert_eq!(state, expected);
            }
        }
    }
}
