use crate::windows::automation::AutomationError32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decimal32 {
    pub coefficient: u128,
    pub negative: bool,
    pub scale: u8,
}
impl Decimal32 {
    pub(crate) const MAX: u128 = (1u128 << 96) - 1;
    pub(crate) fn decode(bytes: [u8; 16]) -> Result<Self, E> {
        let scale = bytes[2];
        let sign = bytes[3];
        if scale > 28 || sign & !0x80 != 0 {
            return Err(E::InvalidParameter);
        }
        Ok(Self {
            coefficient: (u128::from(u32::from_le_bytes(bytes[4..8].try_into().unwrap())) << 64)
                | u128::from(u64::from_le_bytes(bytes[8..].try_into().unwrap())),
            negative: sign != 0,
            scale,
        })
    }
    pub(crate) fn encode(self) -> Result<[u8; 16], E> {
        if self.coefficient > Self::MAX || self.scale > 28 {
            return Err(E::Overflow);
        }
        let mut bytes = [0; 16];
        bytes[..2].copy_from_slice(&14u16.to_le_bytes());
        bytes[2] = self.scale;
        bytes[3] = if self.negative { 0x80 } else { 0 };
        bytes[4..8].copy_from_slice(&((self.coefficient >> 64) as u32).to_le_bytes());
        bytes[8..].copy_from_slice(&(self.coefficient as u64).to_le_bytes());
        Ok(bytes)
    }
    pub(crate) fn rounded(self) -> Result<i128, E> {
        let denominator = 10u128.pow(u32::from(self.scale));
        let mut magnitude = self.coefficient / denominator;
        let remainder = self.coefficient % denominator;
        if remainder > denominator / 2 || (remainder * 2 == denominator && magnitude & 1 != 0) {
            magnitude = magnitude.checked_add(1).ok_or(E::Overflow)?;
        }
        let magnitude = i128::try_from(magnitude).map_err(|_| E::Overflow)?;
        Ok(if self.negative { -magnitude } else { magnitude })
    }
    pub(crate) fn normalized(mut self) -> Self {
        while self.scale != 0 && self.coefficient % 10 == 0 {
            self.coefficient /= 10;
            self.scale -= 1;
        }
        self
    }
    pub(crate) fn text(self) -> String {
        let value = self.normalized();
        let mut digits = value.coefficient.to_string();
        let scale = usize::from(value.scale);
        if scale != 0 {
            if digits.len() <= scale {
                digits.insert_str(0, &"0".repeat(scale + 1 - digits.len()));
            }
            digits.insert(digits.len() - scale, '.');
        }
        if value.negative && value.coefficient != 0 {
            digits.insert(0, '-');
        }
        digits
    }
    pub(super) fn from_float_text(text: &str) -> Result<Self, E> {
        let negative = text.starts_with('-');
        let text = text.trim_start_matches('-');
        let (mantissa, exponent) =
            text.split_once('E')
                .map_or((text, 0), |(mantissa, exponent)| {
                    (
                        mantissa,
                        exponent
                            .parse::<i32>()
                            .expect("bounded numeric formatter exponent"),
                    )
                });
        let mut coefficient = 0u128;
        let mut fractional = false;
        let mut scale = -exponent;
        for digit in mantissa.bytes() {
            if digit == b'.' {
                fractional = true;
            } else {
                coefficient = coefficient
                    .checked_mul(10)
                    .and_then(|n| n.checked_add(u128::from(digit - b'0')))
                    .ok_or(E::Overflow)?;
                scale += i32::from(fractional);
            }
        }
        if scale < 0 {
            coefficient = coefficient
                .checked_mul(
                    10u128
                        .checked_pow(scale.unsigned_abs())
                        .ok_or(E::Overflow)?,
                )
                .ok_or(E::Overflow)?;
            scale = 0;
        } else if scale > 28 {
            coefficient = if let Some(divisor) = 10u128.checked_pow((scale - 28) as u32) {
                let quotient = coefficient / divisor;
                let remainder = coefficient % divisor;
                quotient
                    + u128::from(
                        remainder > divisor / 2 || (remainder * 2 == divisor && quotient & 1 != 0),
                    )
            } else {
                0
            };
            scale = 28;
        }
        if coefficient > Self::MAX {
            return Err(E::Overflow);
        }
        Ok(Self {
            coefficient,
            negative: negative && coefficient != 0,
            scale: scale as u8,
        }
        .normalized())
    }
}
