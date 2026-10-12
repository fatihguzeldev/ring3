use super::*;

#[test]
fn directed_overflow_preserves_finite_saturation_and_architectural_status() {
    for (format, maximum, sign, one, two, half) in [
        (
            BinaryFormat::Binary32,
            0x7f7fffff,
            0x80000000,
            0x3f800000,
            0x40000000,
            0x3f000000,
        ),
        (
            BinaryFormat::Binary64,
            0x7fefffffffffffff,
            0x8000000000000000,
            0x3ff0000000000000,
            0x4000000000000000,
            0x3fe0000000000000,
        ),
    ] {
        for negative in [false, true] {
            let a = maximum | if negative { sign } else { 0 };
            for (operation, b) in [
                (BinaryOperation::Add, a),
                (BinaryOperation::Subtract, a ^ sign),
                (BinaryOperation::Multiply, two),
                (BinaryOperation::Divide, half),
            ] {
                for mode in [
                    Rounding::Zero,
                    if negative {
                        Rounding::Up
                    } else {
                        Rounding::Down
                    },
                ] {
                    let result = binary(operation, format, a, b, mode);
                    assert_eq!(result.bits, a, "{format:?}/{operation:?}/{mode:?}");
                    assert_eq!(result.exceptions, OVERFLOW | PRECISION);
                }
            }
        }
        for mode in [
            Rounding::Nearest,
            Rounding::Down,
            Rounding::Up,
            Rounding::Zero,
        ] {
            let exact = binary(BinaryOperation::Multiply, format, maximum, one, mode);
            assert_eq!(exact.bits, maximum);
            assert_eq!(exact.exceptions, 0);
        }
    }
}

#[test]
fn finite_boundary_rounding_is_distinct_from_unbounded_overflow() {
    for (format, maximum, sign, infinity, one, qnan, snan) in [
        (
            BinaryFormat::Binary32,
            0x7f7fffff,
            0x80000000,
            0x7f800000,
            0x3f800000,
            0x7fc01234,
            0x7f801234,
        ),
        (
            BinaryFormat::Binary64,
            0x7fefffffffffffff,
            0x8000000000000000,
            0x7ff0000000000000,
            0x3ff0000000000000,
            0x7ff8000000001234,
            0x7ff0000000001234,
        ),
    ] {
        for negative in [false, true] {
            let s = if negative { sign } else { 0 };
            let near = binary(
                BinaryOperation::Add,
                format,
                maximum | s,
                1 | s,
                Rounding::Zero,
            );
            assert_eq!(near.bits, maximum | s);
            assert_eq!(near.exceptions, PRECISION);
            let overflow = binary(
                BinaryOperation::Add,
                format,
                maximum | s,
                maximum | s,
                Rounding::Nearest,
            );
            assert_eq!(overflow.bits, infinity | s);
            assert_eq!(overflow.exceptions, OVERFLOW | PRECISION);
        }
        let exact = binary(BinaryOperation::Add, format, infinity, one, Rounding::Zero);
        assert_eq!(exact.bits, infinity);
        assert_eq!(exact.exceptions, 0);
        let quiet = binary(BinaryOperation::Add, format, qnan, one, Rounding::Zero);
        assert_eq!(quiet.bits, qnan);
        assert_eq!(quiet.exceptions, 0);
        let signaling = binary(BinaryOperation::Add, format, snan, one, Rounding::Zero);
        assert_eq!(signaling.exceptions, INVALID);
        let invalid = binary(
            BinaryOperation::Subtract,
            format,
            infinity,
            infinity,
            Rounding::Zero,
        );
        assert_eq!(invalid.exceptions, INVALID);
    }
}
