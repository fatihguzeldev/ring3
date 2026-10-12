use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Double, IeeeFloat, Semantics, Single},
};

use super::{INVALID, OVERFLOW, PRECISION, Rounding, UNDERFLOW, ZERO_DIVIDE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryFormat {
    Binary32,
    Binary64,
}

impl BinaryFormat {
    #[inline]
    pub(crate) fn parameters(self) -> (u64, u64, u64, u32, i32) {
        match self {
            Self::Binary32 => (0x8000_0000, 0x7f80_0000, 0x007f_ffff, 23, 127),
            Self::Binary64 => (
                0x8000_0000_0000_0000,
                0x7ff0_0000_0000_0000,
                0x000f_ffff_ffff_ffff,
                52,
                1023,
            ),
        }
    }
}

#[inline]
pub(crate) fn exact_normal_product(a: u64, b: u64, format: BinaryFormat) -> Option<u64> {
    let (sign, exponent_mask, fraction_mask, fraction_bits, bias) = format.parameters();
    let a_exponent = ((a & exponent_mask) >> fraction_bits) as i32;
    let b_exponent = ((b & exponent_mask) >> fraction_bits) as i32;
    let maximum = (exponent_mask >> fraction_bits) as i32;
    if a_exponent == 0
        || a_exponent == maximum
        || b_exponent == 0
        || b_exponent == maximum
        || (a & fraction_mask != 0 && b & fraction_mask != 0)
    {
        return None;
    }
    let exponent = a_exponent + b_exponent - bias;
    if !(1..maximum).contains(&exponent) {
        return None;
    }
    // normal power-of-two scaling changes only the exponent and sign, without rounding.
    Some(((a ^ b) & sign) | ((exponent as u64) << fraction_bits) | ((a | b) & fraction_mask))
}
#[inline]
pub(crate) fn exact_normal_sum(
    a: u64,
    b: u64,
    format: BinaryFormat,
    subtract: bool,
) -> Option<u64> {
    let (sign, exponent_mask, fraction_mask, fraction_bits, _) = format.parameters();
    let a_exponent = ((a & exponent_mask) >> fraction_bits) as i32;
    let b_exponent = ((b & exponent_mask) >> fraction_bits) as i32;
    let maximum = (exponent_mask >> fraction_bits) as i32;
    if !(1..maximum).contains(&a_exponent) || !(1..maximum).contains(&b_exponent) {
        return None;
    }
    let distance = (a_exponent - b_exponent).unsigned_abs();
    if distance > fraction_bits {
        return None;
    }
    let mut a_significand = (1u64 << fraction_bits) | (a & fraction_mask);
    let mut b_significand = (1u64 << fraction_bits) | (b & fraction_mask);
    let smaller = if a_exponent < b_exponent {
        &mut a_significand
    } else {
        &mut b_significand
    };
    if *smaller & ((1u64 << distance) - 1) != 0 {
        return None;
    }
    *smaller >>= distance;
    let a_value = if a & sign != 0 {
        -(a_significand as i64)
    } else {
        a_significand as i64
    };
    let b_value = if (b & sign != 0) ^ subtract {
        -(b_significand as i64)
    } else {
        b_significand as i64
    };
    let sum = a_value + b_value;
    if sum == 0 {
        return None;
    }
    let mut magnitude = sum.unsigned_abs();
    let mut exponent = a_exponent.max(b_exponent);
    if magnitude >= 1u64 << (fraction_bits + 1) {
        if magnitude & 1 != 0 {
            return None;
        }
        magnitude >>= 1;
        exponent += 1;
    } else {
        let shift = magnitude.leading_zeros() - (63 - fraction_bits);
        magnitude <<= shift;
        exponent -= shift as i32;
    }
    if !(1..maximum).contains(&exponent) {
        return None;
    }
    // exact alignment and normalization leave no rounding or exception flags.
    let result_sign = if sum < 0 { sign } else { 0 };
    Some(result_sign | ((exponent as u64) << fraction_bits) | (magnitude & fraction_mask))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FloatResult {
    pub bits: u64,
    pub exceptions: u16,
}

#[inline]
fn rounding(mode: Rounding) -> Round {
    match mode {
        Rounding::Nearest => Round::NearestTiesToEven,
        Rounding::Down => Round::TowardNegative,
        Rounding::Up => Round::TowardPositive,
        Rounding::Zero => Round::TowardZero,
    }
}

#[inline]
fn exceptions(status: Status) -> u16 {
    (u16::from(status.contains(Status::INVALID_OP)) * INVALID)
        | (u16::from(status.contains(Status::DIV_BY_ZERO)) * ZERO_DIVIDE)
        | (u16::from(status.contains(Status::OVERFLOW)) * OVERFLOW)
        | (u16::from(status.contains(Status::UNDERFLOW)) * UNDERFLOW)
        | (u16::from(status.contains(Status::INEXACT)) * PRECISION)
}

#[inline]
pub(crate) fn binary(
    operation: BinaryOperation,
    format: BinaryFormat,
    a: u64,
    b: u64,
    mode: Rounding,
) -> FloatResult {
    let round = rounding(mode);
    let result = match format {
        BinaryFormat::Binary32 => {
            let x = Single::from_bits(a as u128);
            let y = Single::from_bits(b as u128);
            let r = match operation {
                BinaryOperation::Add => x.add_r(y, round),
                BinaryOperation::Subtract => x.sub_r(y, round),
                BinaryOperation::Multiply => x.mul_r(y, round),
                BinaryOperation::Divide => x.div_r(y, round),
            };
            (r.value.to_bits() as u64, r.status)
        }
        BinaryFormat::Binary64 => {
            let x = Double::from_bits(a as u128);
            let y = Double::from_bits(b as u128);
            let r = match operation {
                BinaryOperation::Add => x.add_r(y, round),
                BinaryOperation::Subtract => x.sub_r(y, round),
                BinaryOperation::Multiply => x.mul_r(y, round),
                BinaryOperation::Divide => x.div_r(y, round),
            };
            (r.value.to_bits() as u64, r.status)
        }
    };
    FloatResult {
        bits: result.0,
        exceptions: exceptions(result.1),
    }
}

#[inline]
pub(crate) fn round_to_integral(bits: u64, format: BinaryFormat, mode: Rounding) -> FloatResult {
    let round = rounding(mode);
    let (bits, status) = match format {
        BinaryFormat::Binary32 => {
            let result = Single::from_bits(bits as u128).round_to_integral(round);
            (result.value.to_bits() as u64, result.status)
        }
        BinaryFormat::Binary64 => {
            let result = Double::from_bits(bits as u128).round_to_integral(round);
            (result.value.to_bits() as u64, result.status)
        }
    };
    FloatResult {
        bits,
        exceptions: exceptions(status),
    }
}

struct WideSingle;
impl Semantics for WideSingle {
    const BITS: usize = 39;
    const EXP_BITS: usize = 15;
}
struct WideDouble;
impl Semantics for WideDouble {
    const BITS: usize = 68;
    const EXP_BITS: usize = 15;
}

#[inline]
pub(crate) fn unbounded_tiny(
    operation: BinaryOperation,
    format: BinaryFormat,
    a: u64,
    b: u64,
    mode: Rounding,
) -> bool {
    let round = rounding(mode);
    let mut loses = false;
    match format {
        BinaryFormat::Binary32 => {
            let x: IeeeFloat<WideSingle> = Single::from_bits(a as u128)
                .convert_r(round, &mut loses)
                .value;
            let y: IeeeFloat<WideSingle> = Single::from_bits(b as u128)
                .convert_r(round, &mut loses)
                .value;
            let value = match operation {
                BinaryOperation::Add => x.add_r(y, round),
                BinaryOperation::Subtract => x.sub_r(y, round),
                BinaryOperation::Multiply => x.mul_r(y, round),
                BinaryOperation::Divide => x.div_r(y, round),
            }
            .value;
            value.is_finite_non_zero() && value.ilogb() < -126
        }
        BinaryFormat::Binary64 => {
            let x: IeeeFloat<WideDouble> = Double::from_bits(a as u128)
                .convert_r(round, &mut loses)
                .value;
            let y: IeeeFloat<WideDouble> = Double::from_bits(b as u128)
                .convert_r(round, &mut loses)
                .value;
            let value = match operation {
                BinaryOperation::Add => x.add_r(y, round),
                BinaryOperation::Subtract => x.sub_r(y, round),
                BinaryOperation::Multiply => x.mul_r(y, round),
                BinaryOperation::Divide => x.div_r(y, round),
            }
            .value;
            value.is_finite_non_zero() && value.ilogb() < -1022
        }
    }
}

#[inline]
pub(crate) fn from_signed32(value: i32, format: BinaryFormat, mode: Rounding) -> FloatResult {
    let round = rounding(mode);
    let result = match format {
        BinaryFormat::Binary32 => {
            let r = Single::from_i128_r(i128::from(value), round);
            (r.value.to_bits() as u64, r.status)
        }
        BinaryFormat::Binary64 => {
            let r = Double::from_i128_r(i128::from(value), round);
            (r.value.to_bits() as u64, r.status)
        }
    };
    FloatResult {
        bits: result.0,
        exceptions: exceptions(result.1),
    }
}

#[inline]
pub(crate) fn to_signed32(bits: u64, format: BinaryFormat, mode: Rounding) -> FloatResult {
    let mut exact = false;
    let round = rounding(mode);
    let r = match format {
        BinaryFormat::Binary32 => Single::from_bits(bits as u128).to_i128_r(32, round, &mut exact),
        BinaryFormat::Binary64 => Double::from_bits(bits as u128).to_i128_r(32, round, &mut exact),
    };
    FloatResult {
        bits: if r.status.contains(Status::INVALID_OP) {
            0x8000_0000
        } else {
            r.value as u64
        },
        exceptions: exceptions(r.status),
    }
}

#[inline]
pub(crate) fn convert(bits: u64, source: BinaryFormat, mode: Rounding) -> FloatResult {
    let mut loses = false;
    let round = rounding(mode);
    let result = match source {
        BinaryFormat::Binary32 => {
            let r: rustc_apfloat::StatusAnd<Double> =
                Single::from_bits(bits as u128).convert_r(round, &mut loses);
            (r.value.to_bits() as u64, r.status)
        }
        BinaryFormat::Binary64 => {
            let source = Double::from_bits(bits as u128);
            let r: rustc_apfloat::StatusAnd<Single> = source.convert_r(round, &mut loses);
            let mut status = r.status;
            if r.value.to_bits() & 0x7fff_ffff == 0x7f7f_ffff
                && status.contains(Status::INEXACT)
                && !status.contains(Status::OVERFLOW)
            {
                // finite saturation can omit vendor overflow metadata; detect it before exponent clamping.
                let unbounded: IeeeFloat<WideSingle> = source.convert_r(round, &mut loses).value;
                if unbounded.is_finite_non_zero() && unbounded.ilogb() > 127 {
                    status |= Status::OVERFLOW | Status::INEXACT;
                }
            }
            (r.value.to_bits() as u64, status)
        }
    };
    FloatResult {
        bits: result.0,
        exceptions: exceptions(result.1),
    }
}

#[inline]
pub(crate) fn narrowing_tiny(bits: u64, mode: Rounding) -> bool {
    let mut loses = false;
    let unbounded: IeeeFloat<WideSingle> = Double::from_bits(bits as u128)
        .convert_r(rounding(mode), &mut loses)
        .value;
    unbounded.is_finite_non_zero() && unbounded.ilogb() < -126
}

#[inline]
pub(crate) fn square_root(bits: u64, format: BinaryFormat, mode: Rounding) -> FloatResult {
    let (sign, exponent_mask, fraction_mask, fraction_bits, bias) = format.parameters();
    if bits & exponent_mask == exponent_mask && bits & fraction_mask != 0 {
        return FloatResult {
            bits: bits | (1 << (fraction_bits - 1)),
            exceptions: u16::from(bits & (1 << (fraction_bits - 1)) == 0),
        };
    }
    if bits & !sign == 0 || bits == exponent_mask {
        return FloatResult {
            bits,
            exceptions: 0,
        };
    }
    if bits & sign != 0 {
        return FloatResult {
            bits: sign | exponent_mask | (1 << (fraction_bits - 1)),
            exceptions: INVALID,
        };
    }
    let mut exponent = ((bits & exponent_mask) >> fraction_bits) as i32 - bias;
    let mut significand = bits & fraction_mask;
    if bits & exponent_mask != 0 {
        significand |= 1 << fraction_bits;
    } else {
        exponent = 1 - bias;
        while significand & (1 << fraction_bits) == 0 {
            significand <<= 1;
            exponent -= 1;
        }
    }
    if exponent & 1 != 0 {
        significand <<= 1;
        exponent -= 1;
    }
    let radicand = u128::from(significand) << fraction_bits;
    let (mut result, remainder) = super::sqrt_rem(radicand);
    let inexact = remainder != 0;
    let increment = match mode {
        Rounding::Nearest => remainder > result,
        Rounding::Up => inexact,
        Rounding::Down | Rounding::Zero => false,
    };
    result += u128::from(increment);
    exponent /= 2;
    if result >= 1u128 << (fraction_bits + 1) {
        result >>= 1;
        exponent += 1;
    }
    FloatResult {
        bits: ((exponent + bias) as u64) << fraction_bits | (result as u64 & fraction_mask),
        exceptions: if inexact { PRECISION } else { 0 },
    }
}

#[cfg(test)]
#[path = "ieee/conversion/native.rs"]
mod conversion;
