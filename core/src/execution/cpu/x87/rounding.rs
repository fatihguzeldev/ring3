use std::cmp::Ordering;

pub(super) fn product_result(result: f64, left: f64, right: f64) -> Ordering {
    let (left, le) = parts(left);
    let (right, re) = parts(right);
    compare(parts(result), (left * right, le + re))
}

pub(super) fn single_product(result: f64, left: f64, right: f64) -> Option<f64> {
    single_rounded(result, |midpoint| product_result(midpoint, left, right))
}

pub(super) fn single_arithmetic_result(
    result: f64,
    left: f64,
    right: f64,
    multiply: bool,
    allow_below_normal: bool,
    toward_zero: bool,
) -> Option<f64> {
    if !multiply {
        single_quotient(result, left, right)
    } else if allow_below_normal && result != 0.0 && result.abs() < f64::from(f32::MIN_POSITIVE) {
        single_product_below_f32_normal(result, left, right, toward_zero)
    } else if toward_zero {
        single_product_toward_zero(result, left, right)
    } else {
        single_product(result, left, right)
    }
}

fn single_product_below_f32_normal(
    result: f64,
    left: f64,
    right: f64,
    toward_zero: bool,
) -> Option<f64> {
    let bits = result.abs().to_bits();
    let lower = bits & !((1_u64 << 29) - 1);
    let remainder = bits - lower;
    let upper = lower.checked_add(1_u64 << 29)?;
    let nearest = match remainder.cmp(&(1_u64 << 28)) {
        Ordering::Less => lower,
        Ordering::Greater => upper,
        Ordering::Equal => match product_result(result.abs(), left, right) {
            Ordering::Greater => lower,
            Ordering::Equal if lower & (1_u64 << 29) == 0 => lower,
            Ordering::Less | Ordering::Equal => upper,
        },
    };
    let nearest = f64::from_bits(nearest);
    if !nearest.is_normal() {
        return None;
    }
    let rounded = if toward_zero && product_result(nearest, left, right) == Ordering::Greater {
        f64::from_bits(nearest.to_bits().checked_sub(1_u64 << 29)?)
    } else {
        nearest
    };
    rounded.is_normal().then(|| rounded.copysign(result))
}

pub(super) fn single_product_toward_zero(result: f64, left: f64, right: f64) -> Option<f64> {
    let nearest = single_product(result, left, right)?;
    if product_result(nearest, left, right) != Ordering::Greater {
        return Some(nearest);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "nearest result already has f32 precision"
    )]
    let narrowed = nearest as f32;
    Some(f64::from(if nearest.is_sign_negative() {
        narrowed.next_up()
    } else {
        narrowed.next_down()
    }))
}

pub(super) fn single_sum(result: f64, left: f64, right: f64) -> Option<f64> {
    single_rounded(result, |midpoint| {
        let compared = sum_result(midpoint.copysign(result), left, right);
        if result.is_sign_negative() {
            compared.reverse()
        } else {
            compared
        }
    })
}

pub(super) fn single_quotient(result: f64, numerator: f64, denominator: f64) -> Option<f64> {
    single_rounded(result, |midpoint| {
        quotient_result(midpoint, numerator, denominator)
    })
}

pub(super) fn single_square_root(result: f64, input: f64) -> Option<f64> {
    single_rounded(result, |midpoint| square_root_result(midpoint, input))
}

pub(super) fn single_sum_toward_zero(result: f64, left: f64, right: f64) -> Option<f64> {
    let nearest = single_sum(result, left, right)?;
    let compared = sum_result(nearest, left, right);
    let away = (nearest.is_sign_positive() && compared == Ordering::Greater)
        || (nearest.is_sign_negative() && compared == Ordering::Less);
    if !away {
        return Some(nearest);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "nearest result already has f32 precision"
    )]
    let narrowed = nearest as f32;
    Some(f64::from(if nearest.is_sign_negative() {
        narrowed.next_up()
    } else {
        narrowed.next_down()
    }))
}

fn single_rounded(result: f64, compare_exact: impl Fn(f64) -> Ordering) -> Option<f64> {
    let magnitude = result.abs();
    if magnitude != 0.0
        && !(f64::from(f32::MIN_POSITIVE)..=f64::from(f32::MAX)).contains(&magnitude)
    {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bounded nearest-even narrowing"
    )]
    let narrowed = magnitude as f32;
    let candidate = f64::from(narrowed);
    let rounded = if candidate.to_bits() == magnitude.to_bits() {
        candidate
    } else {
        let (lower, upper) = if candidate < magnitude {
            (candidate, f64::from(narrowed.next_up()))
        } else {
            (f64::from(narrowed.next_down()), candidate)
        };
        let midpoint = lower + (upper - lower) * 0.5;
        if magnitude.to_bits() == midpoint.to_bits() {
            match compare_exact(midpoint) {
                Ordering::Less => upper,
                Ordering::Greater => lower,
                Ordering::Equal => candidate,
            }
        } else {
            candidate
        }
    };
    Some(rounded.copysign(result))
}

pub(super) fn quotient_result(result: f64, numerator: f64, denominator: f64) -> Ordering {
    product_result(numerator, result, denominator).reverse()
}

pub(super) fn sum_magnitude_result(result: f64, left: f64, right: f64) -> Ordering {
    let rounding = sum_result(result, left, right);
    if result.is_sign_negative() {
        rounding.reverse()
    } else {
        rounding
    }
}

pub(super) fn square_root_result(result: f64, input: f64) -> Ordering {
    product_result(input, result, result).reverse()
}

fn sum_result(result: f64, left: f64, right: f64) -> Ordering {
    let approximated_right = result - left;
    let error = (left - (result - approximated_right)) + (right - approximated_right);
    if error > 0.0 {
        Ordering::Less
    } else if error < 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

fn parts(value: f64) -> (u128, i32) {
    if value == 0.0 {
        return (0, 0);
    }
    let bits = value.to_bits();
    let exponent = i32::try_from((bits >> 52) & 0x7ff).expect("binary64 exponent fits i32");
    (
        u128::from((bits & ((1 << 52) - 1)) | (1 << 52)),
        exponent - 1075,
    )
}

fn compare((left, le): (u128, i32), (right, re): (u128, i32)) -> Ordering {
    if left == 0 || right == 0 {
        return left.cmp(&right);
    }
    let ll = left.leading_zeros();
    let rl = right.leading_zeros();
    let exponent = (le - i32::try_from(ll).expect("leading zeros fits i32"))
        .cmp(&(re - i32::try_from(rl).expect("leading zeros fits i32")));
    exponent.then_with(|| (left << ll).cmp(&(right << rl)))
}
