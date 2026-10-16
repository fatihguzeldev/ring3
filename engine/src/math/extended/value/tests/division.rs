use super::*;

#[test]
fn only_finite_nonzero_division_by_zero_raises_zero_divide() {
    for negative_numerator in [false, true] {
        for negative_denominator in [false, true] {
            let infinity = Ext80::from_parts(negative_numerator, 0x7fff, 1 << 63);
            let zero = Ext80::from_parts(negative_denominator, 0, 0);
            let expected =
                Ext80::from_parts(negative_numerator ^ negative_denominator, 0x7fff, 1 << 63);
            for control in [0x037f, 0x037b] {
                let result = infinity.divide(zero, control);
                assert_eq!(result.value, expected);
                assert_eq!(result.exceptions, 0);
                let finite =
                    Ext80::from_parts(negative_numerator, 0x3fff, 1 << 63).divide(zero, control);
                assert_eq!(finite.value, expected);
                assert_eq!(finite.exceptions, ZERO_DIVIDE);
                let invalid = Ext80::ZERO.divide(zero, control);
                assert_eq!(invalid.value, Ext80::INDEFINITE);
                assert_eq!(invalid.exceptions, INVALID);
            }
        }
    }
}
