use super::*;

const ONE_SIG: u64 = 0x8000_0000_0000_0000;
const SINGLE_UNIT: u64 = 1 << 40;
const ROUNDINGS: [u16; 4] = [0, 0x0400, 0x0800, 0x0c00];

fn assert_result(actual: Rounded, value: Ext80, exceptions: u16, incremented: bool) {
    assert_eq!(actual.value, value);
    assert_eq!(actual.exceptions, exceptions);
    assert_eq!(actual.incremented, incremented);
}

fn increments(rounding: u16, negative: bool, nearest: bool) -> bool {
    match rounding {
        0 => nearest,
        0x0400 => negative,
        0x0800 => !negative,
        0x0c00 => false,
        _ => unreachable!(),
    }
}

fn signed(value: Ext80, negative: bool) -> Ext80 {
    if negative { value.negated() } else { value }
}

#[test]
fn pc24_narrow_division_admits_only_the_selected_significand_domain() {
    for rounding in ROUNDINGS {
        let control = 0x007f | rounding;
        assert_eq!(
            narrow::divide(ONE_SIG, ONE_SIG, control),
            Some(1_u128 << 66),
            "exact normalized quotient"
        );
        assert_eq!(
            narrow::divide(ONE_SIG, 0xc000_0000_0000_0000, control),
            Some(0x0555_5555_u128 << 40),
            "two thirds retains the quotient tail as sticky"
        );
        for (a, b) in [(ONE_SIG | 1, ONE_SIG), (ONE_SIG, ONE_SIG | 1)] {
            assert!(narrow::divide(a, b, control).is_none());
        }
    }
    for control in [0x017f, 0x027f, 0x037f] {
        assert!(narrow::divide(ONE_SIG, ONE_SIG, control).is_none());
    }
}

#[test]
fn pc24_division_has_literal_signed_rounding_and_extended_exponent_results() {
    for rounding in ROUNDINGS {
        for negative_a in [false, true] {
            for negative_b in [false, true] {
                let negative = negative_a ^ negative_b;
                assert_result(
                    signed(Ext80::from_i64(6), negative_a)
                        .divide(signed(Ext80::from_i64(3), negative_b), 0x007f | rounding),
                    Ext80::from_parts(negative, 0x4000, ONE_SIG),
                    0,
                    false,
                );
                for (a_exponent, denominator, exponent, truncated, nearest) in [
                    (0x3fff, 3, 0x3ffd, 0xaaaa_aa00_0000_0000, true),
                    (0x3fff, 25, 0x3ffa, 0xa3d7_0a00_0000_0000, false),
                    (0x4f9f, 3, 0x4f9d, 0xaaaa_aa00_0000_0000, true),
                ] {
                    let incremented = increments(rounding, negative, nearest);
                    for mask in [0x007f, 0x007f & !PRECISION] {
                        assert_result(
                            Ext80::from_parts(negative_a, a_exponent, ONE_SIG).divide(
                                signed(Ext80::from_i64(denominator), negative_b),
                                mask | rounding,
                            ),
                            Ext80::from_parts(
                                negative,
                                exponent,
                                truncated + if incremented { SINGLE_UNIT } else { 0 },
                            ),
                            PRECISION,
                            incremented,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn pc24_division_preserves_literal_range_ties_masks_and_denormal_operands() {
    for negative in [false, true] {
        for rounding in ROUNDINGS {
            let control = 0x007f | rounding;
            // at this precision the smallest extended subnormal is 2^-16405.
            // these normal operands produce exactly 0.5 and 1.5 of that unit.
            for (significand, divisor_exponent, lower, nearest) in [
                (ONE_SIG, 0x4017, 0, false),
                (0xc000_0000_0000_0000, 0x4016, SINGLE_UNIT, true),
            ] {
                let incremented = increments(rounding, negative, nearest);
                assert_result(
                    Ext80::from_parts(negative, 1, significand)
                        .divide(Ext80::from_parts(false, divisor_exponent, ONE_SIG), control),
                    Ext80::from_parts(
                        negative,
                        0,
                        lower + if incremented { SINGLE_UNIT } else { 0 },
                    ),
                    UNDERFLOW | PRECISION,
                    incremented,
                );
            }
            assert_result(
                Ext80::from_parts(negative, 1, ONE_SIG)
                    .divide(Ext80::from_parts(false, 0x4000, ONE_SIG), control),
                Ext80::from_parts(negative, 0, 0x4000_0000_0000_0000),
                0,
                false,
            );
            assert_result(
                Ext80::from_parts(negative, 1, ONE_SIG).divide(
                    Ext80::from_parts(false, 0x4017, ONE_SIG),
                    control & !UNDERFLOW,
                ),
                Ext80::from_parts(negative, 0x5fe9, ONE_SIG),
                UNDERFLOW,
                false,
            );
            let infinity =
                rounding == 0 || rounding == 0x0800 && !negative || rounding == 0x0400 && negative;
            assert_result(
                Ext80::from_parts(negative, 0x7ffe, ONE_SIG)
                    .divide(Ext80::from_parts(false, 0x3ffe, ONE_SIG), control),
                Ext80::from_parts(
                    negative,
                    if infinity { 0x7fff } else { 0x7ffe },
                    if infinity {
                        ONE_SIG
                    } else {
                        0xffff_ff00_0000_0000
                    },
                ),
                OVERFLOW | PRECISION,
                infinity,
            );
            assert_result(
                Ext80::from_parts(negative, 0x7ffe, ONE_SIG).divide(
                    Ext80::from_parts(false, 0x3ffe, ONE_SIG),
                    control & !OVERFLOW,
                ),
                Ext80::from_parts(negative, 0x1fff, ONE_SIG),
                OVERFLOW,
                false,
            );
            let denormal = Ext80::from_parts(negative, 0, 0x4000_0000_0000_0000);
            assert_result(
                denormal.divide(Ext80::ONE, control),
                denormal,
                DENORMAL,
                false,
            );
            assert_result(
                denormal.divide(denormal.absolute(), control),
                Ext80::from_parts(negative, 0x3fff, ONE_SIG),
                DENORMAL,
                false,
            );
            assert_result(
                Ext80::from_parts(negative, 0, ONE_SIG).divide(Ext80::ONE, control),
                Ext80::from_parts(negative, 1, ONE_SIG),
                0,
                false,
            );
        }
    }
}

fn wide_reference(a: Ext80, b: Ext80, control: u16) -> Rounded {
    let Kind::Finite {
        significand: av,
        exponent: ae,
    } = a.kind()
    else {
        panic!("finite test numerator");
    };
    let Kind::Finite {
        significand: bv,
        exponent: be,
    } = b.kind()
    else {
        panic!("finite test denominator");
    };
    round(
        a.negative() ^ b.negative(),
        ae - be - i32::from(av < bv),
        wide_divide(av, bv),
        control,
        a.operand_exceptions() | b.operand_exceptions(),
    )
}

#[test]
fn pc24_division_matches_the_preserved_wide_algorithm_for_bounded_finite_inputs() {
    let exponents = [1, 2, 0x0fff, 0x3ffe, 0x3fff, 0x4000, 0x6fff, 0x7ffe];
    let mut seed = 0xd1b5_4a32_d192_ed03_u64;
    for index in 0..64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let a_sig = ((seed >> 40) | 0x80_0000) << 40;
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let b_sig = ((seed >> 40) | 0x80_0000) << 40;
        let mask = match index % 4 {
            0 => 0x007f,
            1 => 0x007f & !PRECISION,
            2 => 0x007f & !OVERFLOW,
            _ => 0x007f & !UNDERFLOW,
        };
        for negative in [false, true] {
            let a = Ext80::from_parts(negative, exponents[index % exponents.len()], a_sig);
            let b = Ext80::from_parts(
                index & 1 != 0,
                exponents[(index * 3 + 1) % exponents.len()],
                b_sig,
            );
            for rounding in ROUNDINGS {
                let control = mask | rounding;
                let actual = a.divide(b, control);
                let expected = wide_reference(a, b, control);
                assert_eq!(
                    actual.value, expected.value,
                    "a={a:?} b={b:?} control={control:x}"
                );
                assert_eq!(
                    actual.exceptions, expected.exceptions,
                    "a={a:?} b={b:?} control={control:x}"
                );
                assert_eq!(
                    actual.incremented, expected.incremented,
                    "a={a:?} b={b:?} control={control:x}"
                );
            }
        }
    }
}
