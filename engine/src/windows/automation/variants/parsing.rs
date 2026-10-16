use super::{Decimal32, Number32};
use crate::{
    math::extended::{Ext80, Kind},
    windows::automation::AutomationError32 as E,
};

pub(crate) struct NumberSymbols32 {
    pub decimal: String,
    pub group: String,
    pub negative: String,
    pub currency: String,
}
impl Number32 {
    pub(crate) fn parse(units: &[u16], symbols: &NumberSymbols32) -> Result<Self, E> {
        let text = String::from_utf16(units).map_err(|_| E::TypeMismatch)?;
        let mut text = text.trim();
        let mut negative = false;
        if text.starts_with('(') && text.ends_with(')') {
            negative = true;
            text = text[1..text.len() - 1].trim();
        }
        if !symbols.currency.is_empty() {
            if let Some(rest) = text.strip_prefix(&symbols.currency) {
                text = rest.trim_start();
            }
            if let Some(rest) = text.strip_suffix(&symbols.currency) {
                text = rest.trim_end();
            }
        }
        if !symbols.negative.is_empty() && text.starts_with(&symbols.negative) {
            negative = !negative;
            text = &text[symbols.negative.len()..];
        } else if let Some(rest) = text.strip_prefix('-') {
            negative = !negative;
            text = rest;
        } else if let Some(rest) = text.strip_prefix('+') {
            text = rest;
        }
        if text.is_empty() {
            return Err(E::TypeMismatch);
        }
        let mut coefficient = 0u128;
        let mut extended = Ext80::ZERO;
        let mut exact = true;
        let mut digits = 0usize;
        let mut scale = 0i32;
        let mut fractional = false;
        let mut exponent = 0i32;
        while !text.is_empty() {
            if let Some(character) = text.chars().next().filter(char::is_ascii_digit) {
                let digit = u128::from(character as u8 - b'0');
                if exact {
                    if let Some(value) = coefficient
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(digit))
                    {
                        coefficient = value;
                    } else {
                        exact = false;
                        extended = Self::unsigned(coefficient);
                    }
                }
                if !exact {
                    extended = extended
                        .multiply(Ext80::from_i64(10), 0x037f)
                        .value
                        .add(Ext80::from_i64(digit as i64), 0x037f)
                        .value;
                }
                digits += 1;
                if fractional {
                    scale = scale.checked_add(1).ok_or(E::Overflow)?;
                }
                text = &text[1..];
            } else if !fractional
                && !symbols.decimal.is_empty()
                && text.starts_with(&symbols.decimal)
            {
                fractional = true;
                text = &text[symbols.decimal.len()..];
            } else if !fractional
                && !symbols.group.is_empty()
                && text.starts_with(&symbols.group)
                && digits != 0
            {
                text = &text[symbols.group.len()..];
                if !text.starts_with(|c: char| c.is_ascii_digit()) {
                    return Err(E::TypeMismatch);
                }
            } else if digits != 0
                && (text.starts_with('e')
                    || text.starts_with('E')
                    || text.starts_with('d')
                    || text.starts_with('D'))
            {
                exponent = text[1..].parse::<i32>().map_err(|_| E::TypeMismatch)?;
                if exponent.unsigned_abs() > 32768 {
                    return Err(E::Overflow);
                }
                text = "";
            } else {
                return Err(E::TypeMismatch);
            }
        }
        if digits == 0 {
            return Err(E::TypeMismatch);
        }
        scale = scale.checked_sub(exponent).ok_or(E::Overflow)?;
        if exact && scale <= 0 {
            if let Some(power) = 10u128.checked_pow(scale.unsigned_abs()) {
                if let Some(value) = coefficient
                    .checked_mul(power)
                    .and_then(|n| i128::try_from(n).ok())
                {
                    return Ok(Self::Integer(if negative { -value } else { value }));
                }
            }
        }
        if exact && (0..=28).contains(&scale) && coefficient <= Decimal32::MAX {
            return Ok(Self::Decimal(Decimal32 {
                coefficient,
                negative,
                scale: scale as u8,
            }));
        }
        if exact {
            // Splitting a large positive coefficient keeps u64 and UI8 values exact.
            extended = Self::unsigned(coefficient);
        }
        let mut power = Ext80::from_i64(10);
        let mut factor = Ext80::ONE;
        let mut exponent = scale.unsigned_abs();
        while exponent != 0 {
            if exponent & 1 != 0 {
                factor = factor.multiply(power, 0x037f).value;
            }
            exponent >>= 1;
            if exponent != 0 {
                power = power.multiply(power, 0x037f).value;
            }
        }
        extended = if scale < 0 {
            extended.multiply(factor, 0x037f).value
        } else {
            extended.divide(factor, 0x037f).value
        };
        if !matches!(extended.kind(), Kind::Finite { .. } | Kind::Zero) {
            return Err(E::Overflow);
        }
        Ok(Self::Float {
            value: if negative {
                extended.negated()
            } else {
                extended
            },
            digits: 15,
        })
    }
}
