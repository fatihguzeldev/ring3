use super::*;
use crate::math::{INVALID, OVERFLOW, PRECISION, UNDERFLOW};

#[test]
fn every_half_pattern_widens_and_roundtrips_with_only_signaling_nan_quieting() {
    for bits in 0..=u16::MAX {
        let nan = bits & 0x7c00 == 0x7c00 && bits & 0x03ff != 0;
        let signaling = nan && bits & 0x0200 == 0;
        let widened = half_to_single(bits);
        assert_eq!(
            widened.exceptions,
            u16::from(signaling) * INVALID,
            "{bits:04x}"
        );
        let narrowed = single_to_half(widened.bits as u32, Rounding::Nearest);
        assert_eq!(
            narrowed.bits,
            u64::from(if nan { bits | 0x0200 } else { bits }),
            "{bits:04x}"
        );
        assert_eq!(narrowed.exceptions, 0, "{bits:04x}");
    }
}

#[test]
fn literal_half_boundaries_and_nan_payloads_do_not_depend_on_host_float() {
    for (half, single, flags) in [
        (0x0000, 0x0000_0000, 0),
        (0x8000, 0x8000_0000, 0),
        (0x0001, 0x3380_0000, 0),
        (0x03ff, 0x387f_c000, 0),
        (0x0400, 0x3880_0000, 0),
        (0x3c00, 0x3f80_0000, 0),
        (0x7bff, 0x477f_e000, 0),
        (0x7c00, 0x7f80_0000, 0),
        (0xfc00, 0xff80_0000, 0),
        (0x7c01, 0x7fc0_2000, INVALID),
        (0xfe55, 0xffca_a000, 0),
    ] {
        assert_eq!(
            half_to_single(half),
            FloatResult {
                bits: single,
                exceptions: flags
            }
        );
    }
    for (single, half, flags) in [
        (0x3380_0000, 0x0001, 0),
        (0x3300_0000, 0x0000, UNDERFLOW | PRECISION),
        (0x3380_0001, 0x0001, UNDERFLOW | PRECISION),
        (0x4780_0000, 0x7c00, OVERFLOW | PRECISION),
        (0x7f80_0001, 0x7e00, INVALID),
        (0xffc0_0001, 0xfe00, 0),
    ] {
        assert_eq!(
            single_to_half(single, Rounding::Nearest),
            FloatResult {
                bits: half,
                exceptions: flags
            }
        );
    }
}

#[test]
fn directed_rounding_controls_ties_signed_zero_and_overflow() {
    for (mode, positive, negative, overflow) in [
        (Rounding::Nearest, 0x3c00, 0xbc00, 0x7c00),
        (Rounding::Down, 0x3c00, 0xbc01, 0x7bff),
        (Rounding::Up, 0x3c01, 0xbc00, 0x7c00),
        (Rounding::Zero, 0x3c00, 0xbc00, 0x7bff),
    ] {
        assert_eq!(
            single_to_half(0x3f80_1000, mode),
            FloatResult {
                bits: positive,
                exceptions: PRECISION
            }
        );
        assert_eq!(
            single_to_half(0xbf80_1000, mode),
            FloatResult {
                bits: negative,
                exceptions: PRECISION
            }
        );
        assert_eq!(
            single_to_half(0x4780_0000, mode),
            FloatResult {
                bits: overflow,
                exceptions: OVERFLOW | PRECISION
            }
        );
        assert_eq!(
            single_to_half(0x8000_0000, mode),
            FloatResult {
                bits: 0x8000,
                exceptions: 0
            }
        );
    }
}

#[test]
fn tininess_uses_unbounded_half_precision_before_exponent_clamping() {
    for (bits, mode, tiny) in [
        (0x0000_0000, Rounding::Nearest, false),
        (0x3380_0000, Rounding::Nearest, true),
        (0x3300_0000, Rounding::Nearest, true),
        (0x3880_0000, Rounding::Nearest, false),
        (0x387f_ffff, Rounding::Nearest, false),
        (0x387f_ffff, Rounding::Zero, true),
        (0x7fc0_0000, Rounding::Nearest, false),
    ] {
        assert_eq!(half_narrowing_tiny(bits, mode), tiny, "{bits:08x} {mode:?}");
    }
}

#[test]
fn finite_overflow_saturation_reports_range_loss_without_flagging_adjacent_inexact_results() {
    for (bits, mode, expected, flags) in [
        (0x4780_0000, Rounding::Zero, 0x7bff, OVERFLOW | PRECISION),
        (0x4780_0000, Rounding::Down, 0x7bff, OVERFLOW | PRECISION),
        (0xc780_0000, Rounding::Up, 0xfbff, OVERFLOW | PRECISION),
        (0xc780_0000, Rounding::Zero, 0xfbff, OVERFLOW | PRECISION),
        (0x477f_ff00, Rounding::Zero, 0x7bff, PRECISION),
        (0xc77f_ff00, Rounding::Zero, 0xfbff, PRECISION),
        (0x477f_e000, Rounding::Zero, 0x7bff, 0),
        (0x7f80_0000, Rounding::Zero, 0x7c00, 0),
        (0xff80_0000, Rounding::Zero, 0xfc00, 0),
    ] {
        assert_eq!(
            single_to_half(bits, mode),
            FloatResult {
                bits: expected,
                exceptions: flags
            }
        );
    }
}
