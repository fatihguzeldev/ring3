use super::{
    BinaryFormat, Double, Float, FloatConvert, FloatResult, IeeeFloat, Rounding, Single, Status,
    WideDouble, WideSingle, exceptions, rounding,
};

pub(crate) fn fused_multiply_add(
    format: BinaryFormat,
    a: u64,
    b: u64,
    c: u64,
    mode: Rounding,
) -> FloatResult {
    let round = rounding(mode);
    let (bits, mut status) = match format {
        BinaryFormat::Binary32 => {
            let result = Single::from_bits(u128::from(a)).mul_add_r(
                Single::from_bits(u128::from(b)),
                Single::from_bits(u128::from(c)),
                round,
            );
            (result.value.to_bits() as u64, result.status)
        }
        BinaryFormat::Binary64 => {
            let result = Double::from_bits(u128::from(a)).mul_add_r(
                Double::from_bits(u128::from(b)),
                Double::from_bits(u128::from(c)),
                round,
            );
            (result.value.to_bits() as u64, result.status)
        }
    };
    let (sign, exponent, _, _, maximum) = format.parameters();
    // the backend can omit overflow for finite directed saturation.
    if bits & !sign == exponent - 1
        && status.contains(Status::INEXACT)
        && !status.contains(Status::OVERFLOW)
        && unbounded_exponent(format, a, b, c, mode).is_some_and(|value| value > maximum)
    {
        status |= Status::OVERFLOW | Status::INEXACT;
    }
    FloatResult {
        bits,
        exceptions: exceptions(status),
    }
}

fn unbounded_exponent(format: BinaryFormat, a: u64, b: u64, c: u64, mode: Rounding) -> Option<i32> {
    let round = rounding(mode);
    let mut loses = false;
    match format {
        BinaryFormat::Binary32 => {
            let a: IeeeFloat<WideSingle> = Single::from_bits(u128::from(a))
                .convert_r(round, &mut loses)
                .value;
            let b: IeeeFloat<WideSingle> = Single::from_bits(u128::from(b))
                .convert_r(round, &mut loses)
                .value;
            let c: IeeeFloat<WideSingle> = Single::from_bits(u128::from(c))
                .convert_r(round, &mut loses)
                .value;
            let value = a.mul_add_r(b, c, round).value;
            value.is_finite_non_zero().then(|| value.ilogb())
        }
        BinaryFormat::Binary64 => {
            let a: IeeeFloat<WideDouble> = Double::from_bits(u128::from(a))
                .convert_r(round, &mut loses)
                .value;
            let b: IeeeFloat<WideDouble> = Double::from_bits(u128::from(b))
                .convert_r(round, &mut loses)
                .value;
            let c: IeeeFloat<WideDouble> = Double::from_bits(u128::from(c))
                .convert_r(round, &mut loses)
                .value;
            let value = a.mul_add_r(b, c, round).value;
            value.is_finite_non_zero().then(|| value.ilogb())
        }
    }
}

pub(crate) fn fused_unbounded_tiny(
    format: BinaryFormat,
    a: u64,
    b: u64,
    c: u64,
    mode: Rounding,
) -> bool {
    let minimum = 1 - format.parameters().4;
    unbounded_exponent(format, a, b, c, mode).is_some_and(|value| value < minimum)
}

#[cfg(test)]
mod native;
