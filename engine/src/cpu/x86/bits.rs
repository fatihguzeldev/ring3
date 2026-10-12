#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarWidth {
    Word,
    Dword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitCountKind {
    TrailingZero,
    LeadingZero,
    Population,
}
use super::Register32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitManipulationKind {
    AndNot { left: Register32 },
    Extract { control: Register32 },
    IsolateLowest,
    MaskLowest,
    ResetLowest,
    ZeroHigh { index: Register32 },
    Deposit { value: Register32 },
    ExtractMasked { value: Register32 },
    ShiftLeft { count: Register32 },
    ShiftLogicalRight { count: Register32 },
    ShiftArithmeticRight { count: Register32 },
    RotateRight { count: u8 },
}
