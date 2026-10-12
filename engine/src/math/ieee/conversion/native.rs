use super::*;

#[test]
fn narrowing_overflow_retains_finite_result_and_reports_architectural_metadata() {
    for (input, positive, negative) in [
        (0x47f0_0000_0000_0000, 0x7f7f_ffff, 0xff7f_ffff),
        (0x47f0_0000_0000_0001, 0x7f7f_ffff, 0xff7f_ffff),
        (0x7fef_ffff_ffff_ffff, 0x7f7f_ffff, 0xff7f_ffff),
    ] {
        for (sign, expected) in [(0, positive), (0x8000_0000_0000_0000, negative)] {
            let result = convert(input | sign, BinaryFormat::Binary64, Rounding::Zero);
            assert_eq!(result.bits, expected);
            assert_eq!(result.exceptions, OVERFLOW | PRECISION);
        }
    }
    for (input, mode, literal, flags) in [
        (0x47ef_ffff_e000_0000, Rounding::Zero, 0x7f7f_ffff, 0),
        (
            0x47ef_ffff_e000_0001,
            Rounding::Zero,
            0x7f7f_ffff,
            PRECISION,
        ),
        (
            0x47ef_ffff_f000_0000,
            Rounding::Zero,
            0x7f7f_ffff,
            PRECISION,
        ),
        (
            0x47ef_ffff_f000_0000,
            Rounding::Nearest,
            0x7f80_0000,
            OVERFLOW | PRECISION,
        ),
        (
            0x47f0_0000_0000_0000,
            Rounding::Down,
            0x7f7f_ffff,
            OVERFLOW | PRECISION,
        ),
        (
            0xc7f0_0000_0000_0000,
            Rounding::Up,
            0xff7f_ffff,
            OVERFLOW | PRECISION,
        ),
        (0x7ff0_0000_0000_0000, Rounding::Zero, 0x7f80_0000, 0),
        (0xfff0_0000_0000_0000, Rounding::Zero, 0xff80_0000, 0),
        (0x7ff8_0000_2000_0000, Rounding::Zero, 0x7fc0_0001, 0),
        (0x7ff0_0000_2000_0000, Rounding::Zero, 0x7fc0_0001, INVALID),
    ] {
        let result = convert(input, BinaryFormat::Binary64, mode);
        assert_eq!(result.bits, literal, "{input:x}/{mode:?}");
        assert_eq!(result.exceptions, flags, "{input:x}/{mode:?}");
    }
}
