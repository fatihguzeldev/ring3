use super::*;
use crate::math::{OVERFLOW, PRECISION};

#[test]
fn product_and_sum_round_only_once() {
    for (format, a, b, c, expected) in [
        (
            BinaryFormat::Binary32,
            0x3f80_0001,
            0x3f7f_fffe,
            0xbf80_0000,
            0xa880_0000,
        ),
        (
            BinaryFormat::Binary64,
            0x3ff0_0000_0000_0001,
            0x3fef_ffff_ffff_fffe,
            0xbff0_0000_0000_0000,
            0xb970_0000_0000_0000,
        ),
        (
            BinaryFormat::Binary32,
            0x7f7f_ffff,
            0x4000_0000,
            0xff7f_ffff,
            0x7f7f_ffff,
        ),
        (
            BinaryFormat::Binary64,
            0x7fef_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0xffef_ffff_ffff_ffff,
            0x7fef_ffff_ffff_ffff,
        ),
    ] {
        assert_eq!(
            fused_multiply_add(format, a, b, c, Rounding::Nearest),
            FloatResult {
                bits: expected,
                exceptions: 0
            }
        );
    }
}

#[test]
fn finite_directed_overflow_keeps_overflow_and_precision() {
    for (format, maximum, two, sign) in [
        (
            BinaryFormat::Binary32,
            0x7f7f_ffff,
            0x4000_0000,
            0x8000_0000,
        ),
        (
            BinaryFormat::Binary64,
            0x7fef_ffff_ffff_ffff,
            0x4000_0000_0000_0000,
            0x8000_0000_0000_0000,
        ),
    ] {
        for (negative, modes) in [
            (false, [Rounding::Zero, Rounding::Down]),
            (true, [Rounding::Zero, Rounding::Up]),
        ] {
            let a = maximum | if negative { sign } else { 0 };
            for mode in modes {
                assert_eq!(
                    fused_multiply_add(format, a, two, 0, mode),
                    FloatResult {
                        bits: a,
                        exceptions: OVERFLOW | PRECISION
                    }
                );
            }
        }
    }
}

#[test]
fn unbounded_tininess_preserves_exact_and_cancellation_boundaries() {
    for (format, minimum, half, one, negative_minimum) in [
        (
            BinaryFormat::Binary32,
            0x0080_0000,
            0x3f00_0000,
            0x3f80_0000,
            0x8080_0000,
        ),
        (
            BinaryFormat::Binary64,
            0x0010_0000_0000_0000,
            0x3fe0_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0x8010_0000_0000_0000,
        ),
    ] {
        assert!(fused_unbounded_tiny(
            format,
            minimum,
            half,
            0,
            Rounding::Nearest
        ));
        assert!(!fused_unbounded_tiny(
            format,
            minimum,
            one,
            0,
            Rounding::Nearest
        ));
        assert!(!fused_unbounded_tiny(
            format,
            minimum,
            one,
            negative_minimum,
            Rounding::Nearest
        ));
    }
}
