pub(crate) fn carryless_product(left: u64, mut right: u64) -> u128 {
    let mut left = u128::from(left);
    let mut product = 0;
    for _ in 0..64 {
        product ^= left & 0u128.wrapping_sub(u128::from(right & 1));
        left <<= 1;
        right >>= 1;
    }
    product
}

pub(crate) fn sqrt_rem(mut value: u128) -> (u128, u128) {
    let mut result = 0;
    let mut bit = 1_u128 << 126;
    while bit > value {
        bit >>= 2;
    }
    while bit != 0 {
        if value >= result + bit {
            value -= result + bit;
            result = (result >> 1) + bit;
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }
    (result, value)
}

#[cfg(test)]
mod tests {
    use super::sqrt_rem;

    #[test]
    fn exact_remainder_includes_full_width_boundary() {
        for (value, expected) in [
            (0, (0, 0)),
            (1, (1, 0)),
            (2, (1, 1)),
            (3, (1, 2)),
            (4, (2, 0)),
            (u128::MAX, (u64::MAX as u128, 0x1_ffff_ffff_ffff_fffe)),
        ] {
            assert_eq!(sqrt_rem(value), expected);
        }
    }
}
