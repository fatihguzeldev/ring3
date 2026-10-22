use super::super::{Function, evaluate_precise};
use super::*;

#[test]
fn fixed_product_preserves_full_carries_and_discarded_bits() {
    let one = 1_u128 << SCALE;
    for (a, b, expected, remainder) in [
        (one, one, one, false),
        (one - 1, one, one - 1, false),
        (one - 1, one - 1, one - 2, true),
        (
            0x00fe_dcba_9876_5432_10fe_dcba_9876_5432,
            0x00ab_cdef_0123_4567_89ab_cdef_0123_4567,
            0x00ab_0a75_5677_4f56_0384_6ea7_79d0_21e6,
            true,
        ),
        (
            0x0100_0000_0000_0000_ffff_ffff_ffff_ffff,
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
            0x0123_4567_89ab_cdf0_2468_acf1_3579_bcee,
            true,
        ),
    ] {
        assert_eq!(product(a, b), (expected, remainder));
    }
}

#[test]
fn bounded_sine_matches_arbitrary_precision_bounds_including_status() {
    let mut workspace = Default::default();
    let mut seed = 0x8c4f_a12d_591e_783b_u64;
    let mut inputs = vec![
        Ext80::ONE,
        Ext80::from_parts(false, 16382, u64::MAX),
        Ext80::from_parts(false, 16382, 1 << 63),
        Ext80::from_parts(false, 16383 - 16, 1 << 63),
    ];
    for index in 0..24 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        inputs.push(Ext80::from_parts(
            false,
            16382 - index % 16,
            seed | (1 << 63),
        ));
    }
    for x in inputs {
        for negative in [false, true] {
            let x = if negative { x.negated() } else { x };
            for mode in [0, 0x400, 0x800, 0xc00] {
                let expected = evaluate_precise(
                    Function::Sin,
                    x,
                    Ext80::ZERO,
                    0x37f | mode,
                    0,
                    &mut workspace,
                )
                .expect("independent directed arbitrary-precision bounds");
                for precision in [0, 0x200, 0x300] {
                    let actual = sin(x, 0x7f | precision | mode)
                        .unwrap_or_else(|| panic!("bounded x={x:?}, mode={mode:x}"));
                    assert_eq!(
                        (actual.value, actual.exceptions, actual.incremented),
                        (expected.value, expected.exceptions, expected.incremented),
                        "x={x:?}, mode={mode:x}, precision={precision:x}"
                    );
                }
            }
        }
    }
}

#[test]
fn bounded_sine_declines_unproven_domains() {
    for x in [
        Ext80::ZERO,
        Ext80::INDEFINITE,
        Ext80::from_parts(false, 0x7fff, 1 << 63),
        Ext80::from_parts(false, 16383 + 7, (1 << 63) | 1),
        Ext80::from_parts(false, 16383 - 17, u64::MAX),
        Ext80::from_parts(false, 16383 + 8, 1 << 63),
    ] {
        assert!(sin(x, 0x37f).is_none(), "x={x:?}");
    }
}

#[test]
fn bounded_cosine_matches_arbitrary_precision_bounds_including_status() {
    let mut workspace = Default::default();
    let mut accepted = 0;
    let mut seed = 0xe75a_10bd_429c_638f_u64;
    let mut inputs = vec![
        Ext80::ONE,
        Ext80::from_parts(false, 16382, u64::MAX),
        Ext80::from_parts(false, 16382, 1 << 63),
        Ext80::from_parts(false, 16383 - 16, 1 << 63),
        Ext80::from_parts(false, 16383 - 32, 1 << 63),
        Ext80::from_parts(false, 16383 - 57, 1 << 63),
    ];
    for index in 0..24 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        inputs.push(Ext80::from_parts(
            false,
            16382 - (index * 7) % 57,
            seed | (1 << 63),
        ));
    }
    for x in inputs {
        for negative in [false, true] {
            let x = if negative { x.negated() } else { x };
            for mode in [0, 0x400, 0x800, 0xc00] {
                let expected = evaluate_precise(
                    Function::Cos,
                    x,
                    Ext80::ZERO,
                    0x37f | mode,
                    0,
                    &mut workspace,
                )
                .expect("independent directed arbitrary-precision bounds");
                for precision in [0, 0x200, 0x300] {
                    let Some(actual) = cos(x, 0x7f | precision | mode) else {
                        continue;
                    };
                    accepted += 1;
                    assert_eq!(
                        (actual.value, actual.exceptions, actual.incremented),
                        (expected.value, expected.exceptions, expected.incremented),
                        "x={x:?}, mode={mode:x}, precision={precision:x}"
                    );
                }
            }
        }
    }
    assert!(accepted >= 600, "certified {accepted} of 720 cases");
}

#[test]
fn cosine_near_a_rounding_midpoint_retains_the_precise_path() {
    let mut workspace = Default::default();
    for negative in [false, true] {
        let x = Ext80::from_parts(negative, 16383 - 32, 1 << 63);
        for precision in [0, 0x200, 0x300] {
            let control = 0x7f | precision;
            assert!(cos(x, control).is_none());
            let actual =
                super::super::evaluate(Function::Cos, x, Ext80::ZERO, control, &mut workspace)
                    .unwrap();
            assert_eq!(actual.value, Ext80::ONE);
            assert_eq!(actual.exceptions, PRECISION);
            assert!(actual.incremented);
        }
    }
}

#[test]
fn bounded_cosine_declines_unproven_domains() {
    for x in [
        Ext80::ZERO,
        Ext80::INDEFINITE,
        Ext80::from_parts(false, 0x7fff, 1 << 63),
        Ext80::from_parts(false, 16383 + 7, (1 << 63) | 1),
        Ext80::from_parts(false, 16383 - 58, u64::MAX),
        Ext80::from_parts(false, 16383 + 8, 1 << 63),
    ] {
        assert!(cos(x, 0x37f).is_none(), "x={x:?}");
    }
}

#[test]
fn half_pi_constant_matches_independent_directed_bounds() {
    let mut constants = astro_float_num::Consts::new().unwrap();
    for mode in [
        astro_float_num::RoundingMode::Down,
        astro_float_num::RoundingMode::Up,
    ] {
        let pi = constants.pi(256, mode);
        let (words, _, _, exponent, _) = pi.as_raw_parts().unwrap();
        assert_eq!(exponent, 2);
        let bits = words.len() * astro_float_num::WORD_BIT_SIZE;
        let mut significand = 0;
        for offset in 0..=SCALE as usize {
            let position = bits - 1 - offset;
            significand = (significand << 1)
                | u128::from(
                    (words[position / astro_float_num::WORD_BIT_SIZE]
                        >> (position % astro_float_num::WORD_BIT_SIZE))
                        & 1,
                );
        }
        assert_eq!(significand, HALF_PI);
    }
}

#[test]
fn bounded_reduced_trigonometry_matches_arbitrary_precision_bounds() {
    let mut workspace = Default::default();
    let mut accepted = 0;
    let mut seed = 0x1d54_937b_a602_fec8_u64;
    let mut inputs = vec![
        Ext80::from_parts(false, 16383, (1 << 63) | 1),
        Ext80::from_parts(false, 16383, 0xc000_0000_0000_0000),
        Ext80::from_parts(false, 16384, 1 << 63),
        Ext80::from_parts(false, 16384, 0xc000_0000_0000_0000),
        Ext80::from_parts(false, 16385, 1 << 63),
        Ext80::from_parts(false, 16385, 0xa000_0000_0000_0000),
        Ext80::from_parts(false, 16385, 0xc000_0000_0000_0000),
        Ext80::from_parts(false, 16383 + 7, 1 << 63),
    ];
    for index in 0..24 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        inputs.push(Ext80::from_parts(
            false,
            16384 + index % 6,
            seed | (1 << 63),
        ));
    }
    for x in inputs {
        for negative in [false, true] {
            let x = if negative { x.negated() } else { x };
            for function in [Function::Sin, Function::Cos] {
                for mode in [0, 0x400, 0x800, 0xc00] {
                    let expected =
                        evaluate_precise(function, x, Ext80::ZERO, 0x37f | mode, 0, &mut workspace)
                            .expect("independent directed arbitrary-precision bounds");
                    for precision in [0, 0x200, 0x300] {
                        let control = 0x7f | precision | mode;
                        let actual = match function {
                            Function::Sin => sin(x, control),
                            Function::Cos => cos(x, control),
                            _ => unreachable!(),
                        };
                        let Some(actual) = actual else {
                            continue;
                        };
                        accepted += 1;
                        assert_eq!(
                            (actual.value, actual.exceptions, actual.incremented),
                            (expected.value, expected.exceptions, expected.incremented),
                            "{function:?} x={x:?}, mode={mode:x}, precision={precision:x}"
                        );
                    }
                }
            }
        }
    }
    assert!(accepted >= 1400, "certified {accepted} of 1536 cases");
}

#[test]
fn reduction_tracks_quadrants_and_residual_signs() {
    for (exponent, significand, quadrant, negative) in [
        (16383, 0x8000_0000_0000_0000, 0, false),
        (16384, 0x8000_0000_0000_0000, 1, false),
        (16384, 0xc000_0000_0000_0000, 2, true),
        (16385, 0x8000_0000_0000_0000, 3, true),
        (16385, 0xa000_0000_0000_0000, 3, false),
        (16385, 0xc000_0000_0000_0000, 0, true),
    ] {
        let x = Ext80::from_parts(false, exponent, significand);
        let (residual, actual_quadrant, actual_negative) = reduce(input(x, -16).unwrap()).unwrap();
        assert_eq!((actual_quadrant, actual_negative), (quadrant, negative));
        assert!(residual.low <= residual.high && residual.high <= UNIT);
    }
}

#[test]
fn quarter_turn_neighbors_decline_when_rounding_cannot_be_certified() {
    for exponent in [16383, 16384, 16385] {
        for significand in [0xc90f_daa2_2168_c234, 0xc90f_daa2_2168_c235] {
            let x = Ext80::from_parts(false, exponent, significand);
            for negative in [false, true] {
                let x = if negative { x.negated() } else { x };
                for mode in [0, 0x400, 0x800, 0xc00] {
                    assert!(sin(x, 0x37f | mode).is_none());
                    assert!(cos(x, 0x37f | mode).is_none());
                }
            }
        }
    }
}
