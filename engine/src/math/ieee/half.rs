use rustc_apfloat::{
    Float, FloatConvert,
    ieee::{Half, IeeeFloat, Semantics, Single},
};

use super::{FloatResult, exceptions, rounding};
use crate::math::{OVERFLOW, PRECISION, Rounding};

#[inline]
pub(crate) fn half_to_single(bits: u16) -> FloatResult {
    let mut loses = false;
    let result: rustc_apfloat::StatusAnd<Single> =
        Half::from_bits(u128::from(bits)).convert_r(rounding(Rounding::Nearest), &mut loses);
    FloatResult {
        bits: result.value.to_bits() as u64,
        exceptions: exceptions(result.status),
    }
}

#[inline]
pub(crate) fn single_to_half(bits: u32, mode: Rounding) -> FloatResult {
    let mut loses = false;
    let source = Single::from_bits(u128::from(bits));
    let result: rustc_apfloat::StatusAnd<Half> = source.convert_r(rounding(mode), &mut loses);
    let mut flags = exceptions(result.status);
    // apfloat reports only inexact for finite overflow saturation. the source's
    // exponent proves overflow even when directed rounding keeps a finite result.
    if source.is_finite_non_zero() && source.ilogb() > 15 {
        flags |= OVERFLOW | PRECISION;
    }
    FloatResult {
        bits: result.value.to_bits() as u64,
        exceptions: flags,
    }
}

struct WideHalf;
impl Semantics for WideHalf {
    const BITS: usize = 26;
    const EXP_BITS: usize = 15;
}

#[inline]
pub(crate) fn half_narrowing_tiny(bits: u32, mode: Rounding) -> bool {
    let mut loses = false;
    let unbounded: IeeeFloat<WideHalf> = Single::from_bits(u128::from(bits))
        .convert_r(rounding(mode), &mut loses)
        .value;
    unbounded.is_finite_non_zero() && unbounded.ilogb() < -14
}

#[cfg(test)]
mod tests;
