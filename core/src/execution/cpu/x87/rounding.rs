use std::cmp::Ordering;

pub(super) fn product_result(result: f64, left: f64, right: f64) -> Ordering {
    let (left, le) = parts(left);
    let (right, re) = parts(right);
    compare(parts(result), (left * right, le + re))
}

pub(super) fn single_product(result: f64, left: f64, right: f64) -> Option<f64> {
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
            match product_result(midpoint, left, right) {
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

pub(super) fn square_root_result(result: f64, input: f64) -> Ordering {
    product_result(input, result, result).reverse()
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
