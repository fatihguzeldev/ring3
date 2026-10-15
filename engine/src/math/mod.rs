pub(crate) mod aes;
pub(crate) mod extended;
pub(crate) mod ieee;
mod integer;
mod workspace;

pub(crate) use workspace::NumericalWorkspace;

pub(crate) use integer::{carryless_product, sqrt_rem};

pub(crate) const INVALID: u16 = 1;
pub(crate) const DENORMAL: u16 = 2;
pub(crate) const ZERO_DIVIDE: u16 = 4;
pub(crate) const OVERFLOW: u16 = 8;
pub(crate) const UNDERFLOW: u16 = 16;
pub(crate) const PRECISION: u16 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericalError {
    NumericalFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Nearest,
    Down,
    Up,
    Zero,
}

impl Rounding {
    pub fn from_control(control: u16) -> Self {
        match (control >> 10) & 3 {
            0 => Self::Nearest,
            1 => Self::Down,
            2 => Self::Up,
            _ => Self::Zero,
        }
    }
}
