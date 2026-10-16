use super::{Decimal32, Number32};
use crate::{math::extended::Ext80, windows::automation::AutomationError32 as E};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Variant32(pub [u8; 16]);
impl Variant32 {
    pub(crate) const EMPTY: Self = Self([0; 16]);
    pub(crate) fn new(vartype: u16, payload: &[u8]) -> Self {
        let mut bytes = [0; 16];
        bytes[..2].copy_from_slice(&vartype.to_le_bytes());
        bytes[8..8 + payload.len()].copy_from_slice(payload);
        Self(bytes)
    }
    pub(crate) fn vartype(self) -> u16 {
        u16::from_le_bytes(self.0[..2].try_into().unwrap())
    }
    pub(crate) fn base(self) -> u16 {
        self.vartype() & 0xfff
    }
    pub(crate) fn by_ref(self) -> bool {
        self.vartype() & 0x4000 != 0
    }
    pub(crate) fn array(self) -> bool {
        self.vartype() & 0x2000 != 0
    }
    pub(crate) fn pointer(self) -> u32 {
        u32::from_le_bytes(self.0[8..12].try_into().unwrap())
    }
    pub(crate) fn validate(self) -> Result<(), E> {
        let kind = self.base();
        if self.vartype() & 0x9000 != 0
            || !matches!(kind, 0..=14 | 16..=23 | 36)
            || (self.by_ref() && matches!(kind, 0 | 1))
            || (self.array() && matches!(kind, 0 | 1 | 36))
            || (!self.by_ref() && !self.array() && kind == 12)
        {
            return Err(E::BadVariantType);
        }
        if kind == 36 {
            return Err(E::NotImplemented);
        }
        if kind == 14 && !self.by_ref() && !self.array() {
            Decimal32::decode(self.0)?;
        }
        Ok(())
    }
    pub(crate) fn value_bytes(self) -> usize {
        if self.array() {
            return 4;
        }
        match self.base() {
            0 | 1 => 0,
            16 | 17 => 1,
            2 | 11 | 18 => 2,
            3 | 4 | 8 | 9 | 10 | 13 | 19 | 22 | 23 => 4,
            5 | 6 | 7 | 20 | 21 => 8,
            12 | 14 => 16,
            _ => 0,
        }
    }
    pub(crate) fn cleared(mut self) -> Self {
        self.0[..8].fill(0);
        self
    }
    pub(crate) fn boolean_integer(self, target: u16) -> Option<Self> {
        if self.vartype() != 11 {
            return None;
        }
        let value = i16::from_le_bytes(self.0[8..10].try_into().unwrap());
        let size = match target {
            16 | 17 => 1,
            2 | 18 => 2,
            3 | 19 | 22 | 23 => 4,
            20 | 21 => 8,
            _ => return None,
        };
        Some(Self::new(target, &(i64::from(value)).to_le_bytes()[..size]))
    }
    pub(crate) fn number(self) -> Result<Number32, E> {
        self.validate()?;
        if self.array() || self.by_ref() {
            return Err(E::TypeMismatch);
        }
        let b = &self.0[8..];
        Ok(match self.base() {
            0 => Number32::Integer(0),
            2 | 11 => Number32::Integer(i16::from_le_bytes(b[..2].try_into().unwrap()).into()),
            3 | 22 => Number32::Integer(i32::from_le_bytes(b[..4].try_into().unwrap()).into()),
            4 => Number32::Float {
                value: Ext80::from_f32_bits(u32::from_le_bytes(b[..4].try_into().unwrap())),
                digits: 7,
            },
            5 | 7 => Number32::Float {
                value: Ext80::from_f64_bits(u64::from_le_bytes(b.try_into().unwrap())),
                digits: 15,
            },
            6 => Number32::Decimal(Decimal32 {
                coefficient: u128::from(i64::from_le_bytes(b.try_into().unwrap()).unsigned_abs()),
                negative: i64::from_le_bytes(b.try_into().unwrap()) < 0,
                scale: 4,
            }),
            14 => Number32::Decimal(Decimal32::decode(self.0)?),
            16 => Number32::Integer(i8::from_le_bytes([b[0]]).into()),
            17 => Number32::Integer(b[0].into()),
            18 => Number32::Integer(u16::from_le_bytes(b[..2].try_into().unwrap()).into()),
            19 | 23 => Number32::Integer(u32::from_le_bytes(b[..4].try_into().unwrap()).into()),
            20 => Number32::Integer(i64::from_le_bytes(b.try_into().unwrap()).into()),
            21 => Number32::Integer(u64::from_le_bytes(b.try_into().unwrap()).into()),
            _ => return Err(E::TypeMismatch),
        })
    }
}
