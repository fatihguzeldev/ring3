use super::{Ext80, Kind, Rounded, value};
use crate::math::PRECISION;

const SCALE: u32 = 120;
const UNIT: u128 = 1 << SCALE;

#[derive(Clone, Copy)]
struct Interval {
    low: u128,
    high: u128,
}

const fn factorial(value: u32) -> u128 {
    let mut product = 1;
    let mut factor = 2;
    while factor <= value {
        product *= factor as u128;
        factor += 1;
    }
    product
}

const COEFFICIENTS: [Interval; 16] = {
    let mut coefficients = [Interval { low: 0, high: 0 }; 16];
    let mut index = 0;
    while index < coefficients.len() {
        let denominator = factorial((2 * index + 1) as u32);
        let low = UNIT / denominator;
        coefficients[index] = Interval {
            low,
            high: low + if UNIT % denominator != 0 { 1 } else { 0 },
        };
        index += 1;
    }
    assert!(factorial(33) > UNIT);
    index = 0;
    while index + 1 < coefficients.len() {
        assert!(coefficients[index].low >= coefficients[index + 1].high);
        index += 1;
    }
    coefficients
};

fn product(a: u128, b: u128) -> (u128, bool) {
    let a0 = a as u64;
    let a1 = (a >> 64) as u64;
    let b0 = b as u64;
    let b1 = (b >> 64) as u64;
    let p00 = u128::from(a0) * u128::from(b0);
    let p01 = u128::from(a0) * u128::from(b1);
    let p10 = u128::from(a1) * u128::from(b0);
    let p11 = u128::from(a1) * u128::from(b1);
    let middle = (p00 >> 64) + u128::from(p01 as u64) + u128::from(p10 as u64);
    let high = p11 + (p01 >> 64) + (p10 >> 64) + (middle >> 64);
    (
        (high << (128 - SCALE)) | (u128::from(middle as u64) >> (SCALE - 64)),
        p00 as u64 != 0 || middle as u64 & ((1_u64 << (SCALE - 64)) - 1) != 0,
    )
}

impl Interval {
    fn multiply(self, rhs: Self) -> Self {
        let (low, _) = product(self.low, rhs.low);
        let (high, remainder) = product(self.high, rhs.high);
        Self {
            low,
            high: high + u128::from(remainder),
        }
    }

    fn subtract(self, rhs: Self) -> Self {
        Self {
            low: self.low - rhs.high,
            high: self.high - rhs.low,
        }
    }
}

fn fixed_round(magnitude: u128, negative: bool, control: u16) -> Rounded {
    let high = 127 - magnitude.leading_zeros();
    let shift = high - 66;
    let extended = (magnitude >> shift) | u128::from(magnitude & ((1_u128 << shift) - 1) != 0);
    value::round(
        negative,
        high as i32 - SCALE as i32,
        extended,
        control | 0x300,
        0,
    )
}

pub(super) fn sin(x: Ext80, control: u16) -> Option<Rounded> {
    let Kind::Finite {
        significand,
        exponent,
    } = x.kind()
    else {
        return None;
    };
    if !(-16..=0).contains(&exponent) || exponent == 0 && significand != 1 << 63 {
        return None;
    }
    let input = u128::from(significand) << (57 + exponent);
    let input = Interval {
        low: input,
        high: input,
    };
    let square = input.multiply(input);
    let mut polynomial = COEFFICIENTS[15];
    // successive factorial coefficients keep these nested differences positive for |x| <= 1.
    for coefficient in COEFFICIENTS[..15].iter().rev() {
        polynomial = coefficient.subtract(square.multiply(polynomial));
    }
    let mut result = input.multiply(polynomial);
    // after the negative x^31/31! term, the positive alternating tail is less than 1/33!.
    result.high += 1;
    let lower = fixed_round(result.low, x.negative(), control);
    let upper = fixed_round(result.high, x.negative(), control);
    if lower.value != upper.value {
        return None;
    }
    let value = lower.value;
    let exponent = i32::from(value.exponent_bits()) - 16383;
    let exact = u128::from(value.significand()) << (57 + exponent);
    let incremented = if exact > result.high {
        true
    } else if exact < result.low {
        false
    } else {
        return None;
    };
    Some(Rounded {
        value,
        exceptions: PRECISION,
        incremented,
    })
}

#[cfg(test)]
mod tests;
