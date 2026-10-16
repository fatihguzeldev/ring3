use super::{Decimal32, Variant32};
use crate::{
    math::{
        INVALID, OVERFLOW, Rounding,
        extended::{Ext80, Kind},
    },
    windows::automation::AutomationError32 as E,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Number32 {
    Integer(i128),
    Decimal(Decimal32),
    Float { value: Ext80, digits: usize },
}
impl Number32 {
    pub(super) fn unsigned(value: u128) -> Ext80 {
        if value == 0 {
            return Ext80::ZERO;
        }
        if value <= u128::from(u64::MAX) {
            let value = value as u64;
            let shift = value.leading_zeros();
            return Ext80::from_parts(false, (16383 + 63 - shift) as u16, value << shift);
        }
        let high = Self::unsigned(value >> 64);
        let low = Self::unsigned(value & u128::from(u64::MAX));
        high.multiply(Ext80::from_parts(false, 16383 + 64, 1 << 63), 0x037f)
            .value
            .add(low, 0x037f)
            .value
    }
    pub(crate) fn extended(self) -> Ext80 {
        match self {
            Self::Integer(value) => {
                let value80 = Self::unsigned(value.unsigned_abs());
                if value < 0 {
                    value80.negated()
                } else {
                    value80
                }
            }
            Self::Decimal(value) => {
                let number = Self::unsigned(value.coefficient)
                    .divide(Self::unsigned(10u128.pow(u32::from(value.scale))), 0x037f)
                    .value;
                if value.negative {
                    number.negated()
                } else {
                    number
                }
            }
            Self::Float { value, .. } => value,
        }
    }
    pub(crate) fn integer(self) -> Result<i128, E> {
        match self {
            Self::Integer(value) => Ok(value),
            Self::Decimal(value) => value.rounded(),
            Self::Float { value, .. } => {
                let value = value.round_integer(0x037f).value;
                let magnitude = match value.kind() {
                    Kind::Zero => 0,
                    Kind::Finite {
                        significand,
                        exponent,
                    } if (0..127).contains(&exponent) => {
                        if exponent >= 63 {
                            u128::from(significand) << (exponent - 63) as u32
                        } else {
                            u128::from(significand >> (63 - exponent) as u32)
                        }
                    }
                    _ => return Err(E::Overflow),
                };
                let magnitude = i128::try_from(magnitude).map_err(|_| E::Overflow)?;
                Ok(if value.negative() {
                    -magnitude
                } else {
                    magnitude
                })
            }
        }
    }
    pub(crate) fn convert(self, target: u16) -> Result<Variant32, E> {
        let signed = |bits: u32| -> Result<i128, E> {
            let value = self.integer()?;
            let limit = 1i128 << (bits - 1);
            if !(-limit..limit).contains(&value) {
                return Err(E::Overflow);
            }
            Ok(value)
        };
        let unsigned = |bits: u32| -> Result<u128, E> {
            let value = self.integer()?;
            if value < 0 || value as u128 >= 1u128 << bits {
                return Err(E::Overflow);
            }
            Ok(value as u128)
        };
        Ok(match target {
            0 | 1 => Variant32::new(target, &[]),
            2 => Variant32::new(target, &(signed(16)? as i16).to_le_bytes()),
            3 | 22 => Variant32::new(target, &(signed(32)? as i32).to_le_bytes()),
            16 => Variant32::new(target, &(signed(8)? as i8).to_le_bytes()),
            17 => Variant32::new(target, &(unsigned(8)? as u8).to_le_bytes()),
            18 => Variant32::new(target, &(unsigned(16)? as u16).to_le_bytes()),
            19 | 23 => Variant32::new(target, &(unsigned(32)? as u32).to_le_bytes()),
            20 => Variant32::new(target, &(signed(64)? as i64).to_le_bytes()),
            21 => Variant32::new(target, &(unsigned(64)? as u64).to_le_bytes()),
            11 => Variant32::new(
                target,
                &if matches!(self.extended().kind(), Kind::Zero) {
                    0i16
                } else {
                    -1i16
                }
                .to_le_bytes(),
            ),
            4 | 5 | 7 => {
                let (bits, flags, _) = self.extended().to_ieee(
                    if target == 4 { 8 } else { 11 },
                    if target == 4 { 23 } else { 52 },
                    Rounding::Nearest,
                );
                if flags & (INVALID | OVERFLOW) != 0 {
                    return Err(E::Overflow);
                }
                if target == 7 {
                    let value = f64::from_bits(bits);
                    if !(-657435.0..2958466.0).contains(&value) {
                        return Err(E::Overflow);
                    }
                }
                Variant32::new(
                    target,
                    &bits.to_le_bytes()[..if target == 4 { 4 } else { 8 }],
                )
            }
            6 => {
                let value = self
                    .extended()
                    .multiply(Ext80::from_i64(10000), 0x037f)
                    .value;
                let (bits, flags, _) = value.to_integer(64, Rounding::Nearest);
                if flags & INVALID != 0 {
                    return Err(E::Overflow);
                }
                Variant32::new(6, &bits.to_le_bytes())
            }
            14 => {
                let value = match self {
                    Self::Integer(value) => Decimal32 {
                        coefficient: value.unsigned_abs(),
                        negative: value < 0,
                        scale: 0,
                    },
                    Self::Decimal(value) => value,
                    Self::Float { value, digits } => {
                        if matches!(value.kind(), Kind::Finite { exponent, .. } if exponent >= 96) {
                            return Err(E::Overflow);
                        }
                        let value = float_value(value)?;
                        Decimal32::from_float_text(&format!(
                            "{value:.precision$E}",
                            precision = digits - 1
                        ))?
                    }
                };
                Variant32(value.encode()?)
            }
            _ => return Err(E::BadVariantType),
        })
    }
    pub(crate) fn text(self) -> Result<String, E> {
        Ok(match self {
            Self::Integer(value) => value.to_string(),
            Self::Decimal(value) => value.text(),
            Self::Float { value, digits } => {
                let value = float_value(value)?;
                if value == 0.0 {
                    return Ok("0".into());
                }
                let text = format!("{value:.precision$e}", precision = digits - 1);
                let (mantissa, exponent) = text.split_once('e').unwrap();
                let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
                let exponent: i32 = exponent.parse().unwrap();
                let significant = mantissa.bytes().filter(u8::is_ascii_digit).count() as i32;
                let fixed = (-4..digits as i32).contains(&exponent)
                    || exponent < -4 && significant - exponent - 1 <= digits as i32;
                if fixed {
                    let text = format!(
                        "{value:.precision$}",
                        precision = if exponent < -4 {
                            digits
                        } else {
                            (digits as i32 - exponent - 1).max(0) as usize
                        }
                    );
                    if text.contains('.') {
                        text.trim_end_matches('0').trim_end_matches('.').to_owned()
                    } else {
                        text
                    }
                } else {
                    format!("{mantissa}E{exponent:+03}")
                }
            }
        })
    }
}

fn float_value(value: Ext80) -> Result<f64, E> {
    let (bits, flags, _) = value.to_ieee(11, 52, Rounding::Nearest);
    let value = f64::from_bits(bits);
    if flags & (INVALID | OVERFLOW) != 0 || !value.is_finite() {
        Err(E::Overflow)
    } else {
        Ok(value)
    }
}
