use super::{Ext80, Kind, Rounded, value};
use crate::math::PRECISION;

const SCALE: u32 = 120;
const UNIT: u128 = 1 << SCALE;
const HALF_PI: u128 = 0x0192_1fb5_4442_d184_6989_8cc5_1701_b839;

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

const fn coefficients<const N: usize>(first: u32) -> [Interval; N] {
    let mut coefficients = [Interval { low: 0, high: 0 }; N];
    let mut index = 0;
    while index < coefficients.len() {
        let denominator = factorial(2 * index as u32 + first);
        let low = UNIT / denominator;
        coefficients[index] = Interval {
            low,
            high: low + if UNIT % denominator != 0 { 1 } else { 0 },
        };
        index += 1;
    }
    index = 0;
    while index + 1 < coefficients.len() {
        assert!(coefficients[index].low >= coefficients[index + 1].high);
        index += 1;
    }
    coefficients
}

const SINE_COEFFICIENTS: [Interval; 16] = coefficients(1);
const COSINE_COEFFICIENTS: [Interval; 17] = coefficients(0);
const _: () = assert!(factorial(33) > UNIT && factorial(34) > UNIT);

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

fn input(x: Ext80, minimum_exponent: i32) -> Option<Interval> {
    let Kind::Finite {
        significand,
        exponent,
    } = x.kind()
    else {
        return None;
    };
    if !(minimum_exponent..=7).contains(&exponent) || exponent == 7 && significand != 1 << 63 {
        return None;
    }
    let input = u128::from(significand) << (57 + exponent);
    Some(Interval {
        low: input,
        high: input,
    })
}

fn reduce(input: Interval) -> Option<(Interval, u32, bool)> {
    if input.high <= UNIT {
        return Some((input, 0, false));
    }
    // pi/2 lies between these adjacent q120 values; every quadrant keeps that uncertainty.
    let turns = (input.low + HALF_PI / 2) / HALF_PI;
    let low = turns * HALF_PI;
    let high = low + turns;
    let (reduced, negative) = if input.low >= high {
        (
            Interval {
                low: input.low - high,
                high: input.high - low,
            },
            false,
        )
    } else if input.high <= low {
        (
            Interval {
                low: low - input.high,
                high: high - input.low,
            },
            true,
        )
    } else {
        return None;
    };
    (reduced.high <= UNIT).then_some((reduced, turns as u32 & 3, negative))
}

fn certify(result: Interval, negative: bool, control: u16) -> Option<Rounded> {
    if result.low < 1 << 66 {
        return None;
    }
    let lower = fixed_round(result.low, negative, control);
    let upper = fixed_round(result.high, negative, control);
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

fn evaluate(x: Ext80, control: u16, cosine: bool) -> Option<Rounded> {
    let (input, quadrant, residual_negative) = reduce(input(x, if cosine { -57 } else { -16 })?)?;
    let square = input.multiply(input);
    let cosine_polynomial = cosine ^ (quadrant & 1 != 0);
    let (result, negative) = if cosine_polynomial {
        let mut polynomial = COSINE_COEFFICIENTS[16];
        for coefficient in COSINE_COEFFICIENTS[..16].iter().rev() {
            polynomial = coefficient.subtract(square.multiply(polynomial));
        }
        // after the positive x^32/32! term, the negative alternating tail is less than 1/34!.
        polynomial.low -= 1;
        (polynomial, quadrant & 2 != 0)
    } else {
        let mut polynomial = SINE_COEFFICIENTS[15];
        // successive factorial coefficients keep these nested differences positive for |x| <= 1.
        for coefficient in SINE_COEFFICIENTS[..15].iter().rev() {
            polynomial = coefficient.subtract(square.multiply(polynomial));
        }
        let mut result = input.multiply(polynomial);
        // after the negative x^31/31! term, the positive alternating tail is less than 1/33!.
        result.high += 1;
        (result, residual_negative ^ matches!(quadrant, 1 | 2))
    };
    certify(result, negative ^ (!cosine && x.negative()), control)
}

pub(super) fn sin(x: Ext80, control: u16) -> Option<Rounded> {
    evaluate(x, control, false)
}

pub(super) fn cos(x: Ext80, control: u16) -> Option<Rounded> {
    evaluate(x, control, true)
}

#[cfg(test)]
mod tests;
