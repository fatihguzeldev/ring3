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
        Ext80::from_parts(false, 16383, (1 << 63) | 1),
        Ext80::from_parts(false, 16383 - 17, u64::MAX),
        Ext80::from_parts(false, 16383 + 1, 1 << 63),
    ] {
        assert!(sin(x, 0x37f).is_none(), "x={x:?}");
    }
}
