pub(super) fn mul_div(number: i32, numerator: i32, denominator: i32) -> u32 {
    if denominator == 0 {
        return u32::MAX;
    }
    let product = i64::from(number) * i64::from(numerator);
    let divisor = i64::from(denominator);
    let magnitude = (product.unsigned_abs() + divisor.unsigned_abs() / 2) / divisor.unsigned_abs();
    let result = if (product < 0) != (divisor < 0) {
        -(magnitude as i64)
    } else {
        magnitude as i64
    };
    i32::try_from(result).map_or(u32::MAX, |value| value as u32)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_halves_round_away_from_zero_and_overflow_fails() {
        for (a, b, c, want) in [
            (1, 1, 2, 1),
            (-1, 1, 2, -1),
            (1, 1, -2, -1),
            (-1, 1, -2, 1),
            (5, 3, 2, 8),
            (-5, 3, 2, -8),
            (i32::MIN, 1, 1, i32::MIN),
        ] {
            assert_eq!(mul_div(a, b, c), want as u32);
        }
        assert_eq!(mul_div(i32::MAX, 2, 1), u32::MAX);
        assert_eq!(mul_div(1, 2, 0), u32::MAX);
    }
}
