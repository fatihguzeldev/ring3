use super::*;
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Double, Single, X87DoubleExtended},
};

const MODES: [(X87Rounding, Round); 4] = [
    (X87Rounding::Nearest, Round::NearestTiesToEven),
    (X87Rounding::Down, Round::TowardNegative),
    (X87Rounding::Up, Round::TowardPositive),
    (X87Rounding::Zero, Round::TowardZero),
];

fn increment(mode: X87Rounding, negative: bool, nearest: bool) -> bool {
    match mode {
        X87Rounding::Nearest => nearest,
        X87Rounding::Down => negative,
        X87Rounding::Up => !negative,
        X87Rounding::Zero => false,
    }
}

#[test]
fn normal_ieee_conversion_matches_independent_apfloat_in_every_rounding_mode() {
    let mut seed = 0xe703_7ed1_a0b4_28db_u64;
    for (exponent_bits, fraction_bits, spread) in [(8, 23, 120), (11, 52, 1000)] {
        for _ in 0..64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let exponent = (seed % (2 * spread + 1)) as i32 - spread as i32;
            for negative in [false, true] {
                let value =
                    Ext80::from_parts(negative, (16383 + exponent) as u16, seed | (1 << 63));
                let source = X87DoubleExtended::from_bits(
                    u128::from(value.significand())
                        | (u128::from(u16::from_le_bytes([value.0[8], value.0[9]])) << 64),
                );
                for (mode, rounding) in MODES {
                    let mut loses = false;
                    let (expected, status) = if exponent_bits == 8 {
                        let result: rustc_apfloat::StatusAnd<Single> =
                            source.convert_r(rounding, &mut loses);
                        (result.value.to_bits() as u64, result.status)
                    } else {
                        let result: rustc_apfloat::StatusAnd<Double> =
                            source.convert_r(rounding, &mut loses);
                        (result.value.to_bits() as u64, result.status)
                    };
                    let (actual, exceptions, _) = value.to_ieee(exponent_bits, fraction_bits, mode);
                    assert_eq!(actual, expected, "{value:?} {mode:?} f{fraction_bits}");
                    assert_eq!(
                        exceptions,
                        if status.contains(Status::INEXACT) {
                            PRECISION
                        } else {
                            0
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn ieee_normal_ties_carry_and_overflow_preserve_literal_bits_and_c1() {
    for (exponent_bits, fraction_bits, bias, one) in [
        (8, 23, 127, 0x3f80_0000),
        (11, 52, 1023, 0x3ff0_0000_0000_0000),
    ] {
        let unit = 1_u64 << (63 - fraction_bits);
        for negative in [false, true] {
            let sign = u64::from(negative) << (exponent_bits + fraction_bits);
            for (mode, _) in MODES {
                for (significand, truncated, nearest) in [
                    ((1_u64 << 63) + unit / 2, one, false),
                    ((1_u64 << 63) + unit + unit / 2, one + 1, true),
                    (
                        (u64::MAX & !(unit - 1)) + unit / 2,
                        one + (1_u64 << fraction_bits) - 1,
                        true,
                    ),
                ] {
                    let up = increment(mode, negative, nearest);
                    assert_eq!(
                        Ext80::from_parts(negative, 0x3fff, significand).to_ieee(
                            exponent_bits,
                            fraction_bits,
                            mode
                        ),
                        (sign | (truncated + u64::from(up)), PRECISION, up)
                    );
                }
                let infinity = increment(mode, negative, true);
                let exponent_mask = (1_u64 << exponent_bits) - 1;
                let magnitude = if infinity {
                    exponent_mask << fraction_bits
                } else {
                    ((exponent_mask - 1) << fraction_bits) | ((1_u64 << fraction_bits) - 1)
                };
                assert_eq!(
                    Ext80::from_parts(negative, (16384 + bias) as u16, 1 << 63).to_ieee(
                        exponent_bits,
                        fraction_bits,
                        mode
                    ),
                    (sign | magnitude, OVERFLOW | PRECISION, infinity)
                );
            }
        }
    }
}

#[test]
fn ieee_tiny_promotion_exact_unmasked_underflow_and_pseudo_denormals_are_preserved() {
    for (exponent_bits, fraction_bits, bias) in [(8, 23, 127), (11, 52, 1023)] {
        let min = 1 - bias;
        let min_normal = 1_u64 << fraction_bits;
        for negative in [false, true] {
            let sign = u64::from(negative) << (exponent_bits + fraction_bits);
            for (mode, _) in MODES {
                let up = increment(mode, negative, true);
                assert_eq!(
                    Ext80::from_parts(negative, (16383 + min - 1) as u16, u64::MAX).to_ieee(
                        exponent_bits,
                        fraction_bits,
                        mode
                    ),
                    (
                        sign | (min_normal - 1 + u64::from(up)),
                        PRECISION | if up { 0 } else { UNDERFLOW },
                        up
                    )
                );
                let control = 0x037f
                    | match mode {
                        X87Rounding::Nearest => 0,
                        X87Rounding::Down => 0x400,
                        X87Rounding::Up => 0x800,
                        X87Rounding::Zero => 0xc00,
                    };
                assert_eq!(
                    Ext80::from_parts(negative, (16383 + min - 1) as u16, 1 << 63)
                        .to_ieee_with_control(exponent_bits, fraction_bits, control & !UNDERFLOW),
                    (sign | (min_normal >> 1), UNDERFLOW, false)
                );
                let pseudo = Ext80::from_parts(negative, 0, 1 << 63).to_ieee(
                    exponent_bits,
                    fraction_bits,
                    mode,
                );
                assert_eq!(
                    pseudo,
                    Ext80::from_parts(negative, 1, 1 << 63).to_ieee(
                        exponent_bits,
                        fraction_bits,
                        mode
                    )
                );
                assert_eq!(pseudo.1, UNDERFLOW | PRECISION);
                assert_eq!(
                    Ext80::from_parts(negative, 0, 1)
                        .to_ieee(exponent_bits, fraction_bits, mode)
                        .1,
                    DENORMAL | UNDERFLOW | PRECISION
                );
            }
        }
    }
}

#[test]
fn ieee_nan_quieting_and_unsupported_operands_keep_sign_and_exception_identity() {
    for (exponent_bits, fraction_bits) in [(8, 23), (11, 52)] {
        let infinity = ((1_u64 << exponent_bits) - 1) << fraction_bits;
        let quiet_nan = infinity | (1 << (fraction_bits - 1));
        for negative in [false, true] {
            let sign = u64::from(negative) << (exponent_bits + fraction_bits);
            for (mode, _) in MODES {
                for (exponent, significand, exceptions) in [
                    (0x7fff, 0x8000_0000_0000_0001, INVALID),
                    (0x7fff, 0xc000_0000_0000_0001, 0),
                    (1, 0x4000_0000_0000_0000, INVALID),
                ] {
                    assert_eq!(
                        Ext80::from_parts(negative, exponent, significand).to_ieee(
                            exponent_bits,
                            fraction_bits,
                            mode
                        ),
                        (sign | quiet_nan, exceptions, false)
                    );
                }
                assert_eq!(
                    Ext80::from_parts(negative, 0x7fff, 1 << 63).to_ieee(
                        exponent_bits,
                        fraction_bits,
                        mode
                    ),
                    (sign | infinity, 0, false)
                );
            }
        }
    }
}

#[test]
fn normalized_rounding_preserves_range_edges_incoming_flags_and_u64_carry() {
    for (precision_bits, precision) in [(0, 24), (0x200, 53), (0x300, 64)] {
        for exponent in [-16382, 16382] {
            for negative in [false, true] {
                let exact = round(
                    negative,
                    exponent,
                    1_u128 << 66,
                    0x7f | precision_bits,
                    DENORMAL | ZERO_DIVIDE,
                );
                assert_eq!(
                    exact.value,
                    Ext80::from_parts(negative, (exponent + 16383) as u16, 1 << 63)
                );
                assert_eq!(exact.exceptions, DENORMAL | ZERO_DIVIDE);
                assert!(!exact.incremented);
                let shifted = (((1_u128 << precision) - 1) << (67 - precision))
                    | (1_u128 << (66 - precision));
                let carried = round(
                    negative,
                    exponent,
                    shifted,
                    0x7f | precision_bits,
                    DENORMAL | ZERO_DIVIDE,
                );
                assert_eq!(
                    carried.value,
                    Ext80::from_parts(negative, (exponent + 16384) as u16, 1 << 63)
                );
                assert_eq!(carried.exceptions, DENORMAL | ZERO_DIVIDE | PRECISION);
                assert!(carried.incremented);
            }
        }
    }
}
