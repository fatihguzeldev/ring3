// x87 transcendental instructions use this declared software profile: mathematical
// directed bounds rounded to extended80, rather than one CPU's internal pi approximation.
// all operations stay in integer/arbitrary precision form; no host float conversion is used.
use astro_float_num::{BigFloat, Consts, RoundingMode as R, Sign, WORD_BIT_SIZE};

use super::{
    execute::X87Error,
    value::{self, Ext80, Kind, PRECISION, Rounded, ZERO_DIVIDE},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Function {
    Exp2MinusOne,
    Log2Multiply,
    Log2PlusOneMultiply,
    Atan2,
    Sin,
    Cos,
    Tan,
}

struct Interval {
    low: BigFloat,
    high: BigFloat,
}

impl Interval {
    fn exact(value: BigFloat) -> Self {
        Self {
            low: value.clone(),
            high: value,
        }
    }
    fn add(&self, rhs: &Self, p: usize) -> Self {
        Self {
            low: self.low.add(&rhs.low, p, R::Down),
            high: self.high.add(&rhs.high, p, R::Up),
        }
    }
    fn sub(&self, rhs: &Self, p: usize) -> Self {
        Self {
            low: self.low.sub(&rhs.high, p, R::Down),
            high: self.high.sub(&rhs.low, p, R::Up),
        }
    }
    fn combine(&self, rhs: &Self, p: usize, divide: bool) -> Result<Self, X87Error> {
        let mut low = None;
        let mut high = None;
        for a in [&self.low, &self.high] {
            for b in [&rhs.low, &rhs.high] {
                let down = if divide {
                    a.div(b, p, R::Down)
                } else {
                    a.mul(b, p, R::Down)
                };
                let up = if divide {
                    a.div(b, p, R::Up)
                } else {
                    a.mul(b, p, R::Up)
                };
                if down.is_nan() || up.is_nan() {
                    return Err(X87Error::NumericalFailure);
                }
                if low
                    .as_ref()
                    .is_none_or(|old: &BigFloat| down.cmp(old) == Some(-1))
                {
                    low = Some(down);
                }
                if high
                    .as_ref()
                    .is_none_or(|old: &BigFloat| up.cmp(old) == Some(1))
                {
                    high = Some(up);
                }
            }
        }
        Ok(Self {
            low: low.ok_or(X87Error::NumericalFailure)?,
            high: high.ok_or(X87Error::NumericalFailure)?,
        })
    }
    fn unary(&self, p: usize, constants: &mut Consts, function: Function) -> Self {
        let call = |a: &BigFloat, rm, cc: &mut Consts| match function {
            Function::Exp2MinusOne => a.exp(p, rm, cc),
            Function::Log2Multiply | Function::Log2PlusOneMultiply => a.log2(p, rm, cc),
            Function::Atan2 => a.atan(p, rm, cc),
            _ => unreachable!(),
        };
        Self {
            low: call(&self.low, R::Down, constants),
            high: call(&self.high, R::Up, constants),
        }
    }
}

fn to_big(value: Ext80, p: usize) -> BigFloat {
    match value.kind() {
        Kind::Zero => {
            let zero = BigFloat::from_u64(0, p);
            if value.negative() { zero.neg() } else { zero }
        }
        Kind::Finite {
            significand,
            exponent,
        } => {
            let mut number = BigFloat::from_u64(significand, p);
            number.set_exponent(exponent + 1);
            if value.negative() {
                number.neg()
            } else {
                number
            }
        }
        Kind::Infinity => {
            if value.negative() {
                astro_float_num::INF_NEG
            } else {
                astro_float_num::INF_POS
            }
        }
        _ => astro_float_num::NAN,
    }
}

fn to_extended(value: &BigFloat, control: u16, exceptions: u16) -> Result<Rounded, X87Error> {
    if value.is_nan() {
        return Err(X87Error::NumericalFailure);
    }
    if value.is_inf() {
        return Ok(Rounded::exact(
            Ext80::from_parts(value.is_inf_neg(), 0x7fff, 1 << 63),
            exceptions,
        ));
    }
    let (words, _, sign, exponent, _) = value.as_raw_parts().ok_or(X87Error::NumericalFailure)?;
    if value.is_zero() {
        return Ok(Rounded::exact(
            Ext80::from_parts(sign == Sign::Neg, 0, 0),
            exceptions,
        ));
    }
    let bits = words.len() * WORD_BIT_SIZE;
    let mut extended = 0u128;
    let kept = bits.min(67);
    for offset in 0..kept {
        let position = bits - 1 - offset;
        let bit = (words[position / WORD_BIT_SIZE] >> (position % WORD_BIT_SIZE)) & 1;
        extended = (extended << 1) | u128::from(bit);
    }
    if kept < 67 {
        extended <<= 67 - kept;
    }
    for position in 0..bits.saturating_sub(67) {
        if (words[position / WORD_BIT_SIZE] >> (position % WORD_BIT_SIZE)) & 1 != 0 {
            extended |= 1;
            break;
        }
    }
    Ok(value::round(
        sign == Sign::Neg,
        exponent - 1,
        extended,
        control | 0x300,
        exceptions,
    ))
}

fn pi(constants: &mut Consts, p: usize) -> Interval {
    Interval {
        low: constants.pi(p, R::Down),
        high: constants.pi(p, R::Up),
    }
}

fn atan2(x: Ext80, y: Ext80, p: usize, constants: &mut Consts) -> Result<Interval, X87Error> {
    let two = Interval::exact(BigFloat::from_u64(2, p));
    let four = Interval::exact(BigFloat::from_u64(4, p));
    let angle = if matches!(y.kind(), Kind::Zero) {
        if !x.negative() {
            return Ok(Interval::exact(to_big(y, p)));
        } else {
            pi(constants, p)
        }
    } else if matches!(x.kind(), Kind::Zero)
        || matches!(y.kind(), Kind::Infinity) && !matches!(x.kind(), Kind::Infinity)
    {
        pi(constants, p).combine(&two, p, true)?
    } else if matches!(x.kind(), Kind::Infinity) {
        if matches!(y.kind(), Kind::Infinity) {
            let quarter = pi(constants, p).combine(&four, p, true)?;
            if x.negative() {
                quarter.combine(&Interval::exact(BigFloat::from_u64(3, p)), p, false)?
            } else {
                quarter
            }
        } else if x.negative() {
            pi(constants, p)
        } else {
            return Ok(Interval::exact(to_big(
                Ext80::from_parts(y.negative(), 0, 0),
                p,
            )));
        }
    } else {
        let ratio = Interval::exact(to_big(y.absolute(), p)).combine(
            &Interval::exact(to_big(x.absolute(), p)),
            p,
            true,
        )?;
        let acute = ratio.unary(p, constants, Function::Atan2);
        if x.negative() {
            pi(constants, p).sub(&acute, p)
        } else {
            acute
        }
    };
    Ok(if y.negative() {
        Interval {
            low: angle.high.neg(),
            high: angle.low.neg(),
        }
    } else {
        angle
    })
}

pub(super) fn evaluate(
    function: Function,
    x: Ext80,
    y: Ext80,
    control: u16,
) -> Result<Rounded, X87Error> {
    let operand_exceptions = x.operand_exceptions()
        | if matches!(
            function,
            Function::Log2Multiply | Function::Log2PlusOneMultiply | Function::Atan2
        ) {
            y.operand_exceptions()
        } else {
            0
        };
    if let Some(special) = Ext80::special(
        x,
        if matches!(
            function,
            Function::Log2Multiply | Function::Log2PlusOneMultiply | Function::Atan2
        ) {
            y
        } else {
            Ext80::ZERO
        },
    ) {
        return Ok(special);
    }
    match function {
        Function::Sin | Function::Cos | Function::Tan if matches!(x.kind(), Kind::Infinity) => {
            return Ok(Rounded::invalid());
        }
        Function::Sin | Function::Tan | Function::Exp2MinusOne
            if matches!(x.kind(), Kind::Zero) =>
        {
            return Ok(Rounded::exact(x, operand_exceptions));
        }
        Function::Cos if matches!(x.kind(), Kind::Zero) => {
            return Ok(Rounded::exact(Ext80::ONE, operand_exceptions));
        }
        Function::Exp2MinusOne if x.absolute() == Ext80::ONE => {
            return Ok(Rounded::exact(
                if x.negative() {
                    Ext80::from_parts(true, 16382, 1 << 63)
                } else {
                    Ext80::ONE
                },
                operand_exceptions,
            ));
        }
        Function::Exp2MinusOne if matches!(x.kind(), Kind::Infinity) => {
            return Ok(Rounded::exact(
                if x.negative() {
                    Ext80::ONE.negated()
                } else {
                    x
                },
                operand_exceptions,
            ));
        }
        Function::Log2Multiply if matches!(x.kind(), Kind::Zero) => {
            return Ok(if matches!(y.kind(), Kind::Zero) {
                Rounded::invalid()
            } else {
                Rounded::exact(
                    Ext80::from_parts(!y.negative(), 0x7fff, 1 << 63),
                    operand_exceptions | ZERO_DIVIDE,
                )
            });
        }
        Function::Log2Multiply if x.negative() => return Ok(Rounded::invalid()),
        Function::Log2PlusOneMultiply
            if x.compare(Ext80::ONE.negated(), true).0 == Some(core::cmp::Ordering::Less) =>
        {
            return Ok(Rounded::invalid());
        }
        Function::Log2PlusOneMultiply if x == Ext80::ONE.negated() => {
            return Ok(if matches!(y.kind(), Kind::Zero) {
                Rounded::invalid()
            } else {
                Rounded::exact(
                    Ext80::from_parts(!y.negative(), 0x7fff, 1 << 63),
                    operand_exceptions | ZERO_DIVIDE,
                )
            });
        }
        _ => {}
    }
    if matches!(
        function,
        Function::Log2Multiply | Function::Log2PlusOneMultiply
    ) {
        let log_negative = if function == Function::Log2PlusOneMultiply {
            x.negative()
        } else {
            x.compare(Ext80::ONE, true).0 == Some(core::cmp::Ordering::Less)
        };
        let log_zero = if function == Function::Log2PlusOneMultiply {
            matches!(x.kind(), Kind::Zero)
        } else {
            x == Ext80::ONE
        };
        if log_zero {
            return Ok(if matches!(y.kind(), Kind::Infinity) {
                Rounded::invalid()
            } else {
                Rounded::exact(
                    Ext80::from_parts(y.negative() ^ log_negative, 0, 0),
                    operand_exceptions,
                )
            });
        }
        if matches!(x.kind(), Kind::Infinity) {
            return Ok(if matches!(y.kind(), Kind::Zero) {
                Rounded::invalid()
            } else {
                Rounded::exact(
                    Ext80::from_parts(y.negative(), 0x7fff, 1 << 63),
                    operand_exceptions,
                )
            });
        }
        if matches!(y.kind(), Kind::Zero | Kind::Infinity) {
            return Ok(Rounded::exact(
                Ext80::from_parts(
                    y.negative() ^ log_negative,
                    if matches!(y.kind(), Kind::Infinity) {
                        0x7fff
                    } else {
                        0
                    },
                    if matches!(y.kind(), Kind::Infinity) {
                        1 << 63
                    } else {
                        0
                    },
                ),
                operand_exceptions,
            ));
        }
    }
    let cancellation = if matches!(
        function,
        Function::Exp2MinusOne | Function::Log2PlusOneMultiply
    ) {
        match x.kind() {
            Kind::Finite { exponent, .. } => exponent.saturating_neg().max(0) as usize,
            _ => 0,
        }
    } else {
        0
    };
    let mut constants = Consts::new().map_err(|_| X87Error::NumericalFailure)?;
    for extra in [256, 512, 1024, 2048, 4096] {
        let p = cancellation + extra;
        let input = to_big(x, p);
        let one = Interval::exact(BigFloat::from_u64(1, p));
        let result = match function {
            Function::Sin => Interval {
                low: input.sin(p, R::Down, &mut constants),
                high: input.sin(p, R::Up, &mut constants),
            },
            Function::Cos => Interval {
                low: input.cos(p, R::Down, &mut constants),
                high: input.cos(p, R::Up, &mut constants),
            },
            Function::Tan => Interval {
                low: input.tan(p, R::Down, &mut constants),
                high: input.tan(p, R::Up, &mut constants),
            },
            Function::Exp2MinusOne => {
                let ln2 = Interval {
                    low: constants.ln_2(p, R::Down),
                    high: constants.ln_2(p, R::Up),
                };
                Interval::exact(input)
                    .combine(&ln2, p, false)?
                    .unary(p, &mut constants, function)
                    .sub(&one, p)
            }
            Function::Log2Multiply | Function::Log2PlusOneMultiply => {
                let input = Interval::exact(input);
                let input = if function == Function::Log2PlusOneMultiply {
                    input.add(&one, p)
                } else {
                    input
                };
                input.unary(p, &mut constants, function).combine(
                    &Interval::exact(to_big(y, p)),
                    p,
                    false,
                )?
            }
            Function::Atan2 => atan2(x, y, p, &mut constants)?,
        };
        let low = to_extended(&result.low, control, operand_exceptions)?;
        let high = to_extended(&result.high, control, operand_exceptions)?;
        if low.value == high.value {
            let mut exact = to_big(low.value, p);
            let range = low.exceptions | high.exceptions;
            if range & value::OVERFLOW != 0 && control & value::OVERFLOW == 0 {
                if let Some(exponent) = exact.exponent() {
                    exact.set_exponent(exponent + 24576);
                }
            } else if range & value::UNDERFLOW != 0
                && control & value::UNDERFLOW == 0
                && let Some(exponent) = exact.exponent()
            {
                exact.set_exponent(exponent - 24576);
            }
            let equal_bounds = result.low.cmp(&result.high) == Some(0);
            if !equal_bounds
                && exact.cmp(&result.low) != Some(-1)
                && exact.cmp(&result.high) != Some(1)
            {
                continue;
            }
            let inexact = !equal_bounds || exact.cmp(&result.low) != Some(0);
            let incremented = if low.value.negative() {
                exact.cmp(&result.low) == Some(-1)
            } else {
                exact.cmp(&result.high) == Some(1)
            };
            return Ok(Rounded {
                value: low.value,
                exceptions: low.exceptions | high.exceptions | if inexact { PRECISION } else { 0 },
                incremented,
            });
        }
    }
    Err(X87Error::NumericalFailure)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_values_signed_zero_and_quadrants() {
        let eval = |f, x, y| evaluate(f, x, y, 0x37f).unwrap().value;
        assert_eq!(
            eval(Function::Exp2MinusOne, Ext80::ONE, Ext80::ZERO),
            Ext80::ONE
        );
        assert_eq!(
            eval(
                Function::Log2Multiply,
                Ext80::from_i64(8),
                Ext80::from_i64(3)
            ),
            Ext80::from_i64(9)
        );
        assert_eq!(
            eval(Function::Sin, Ext80::ZERO.negated(), Ext80::ZERO),
            Ext80::ZERO.negated()
        );
        assert_eq!(eval(Function::Cos, Ext80::ZERO, Ext80::ZERO), Ext80::ONE);
        assert_eq!(
            eval(Function::Atan2, Ext80::ONE, Ext80::ZERO.negated()),
            Ext80::ZERO.negated()
        );
        assert_eq!(
            eval(Function::Atan2, Ext80::ONE, Ext80::ONE),
            Ext80::from_parts(false, 16382, 0xc90f_daa2_2168_c235)
        );
    }
    #[test]
    fn tiny_cancellation_retains_extended_range() {
        let tiny = Ext80::from_parts(false, 1, 1 << 63);
        let result = evaluate(Function::Exp2MinusOne, tiny, Ext80::ZERO, 0x37f).unwrap();
        assert_ne!(result.value, Ext80::ZERO);
        let log = evaluate(Function::Log2PlusOneMultiply, tiny, Ext80::ONE, 0x37f).unwrap();
        assert_ne!(log.value, Ext80::ZERO);
    }
}
