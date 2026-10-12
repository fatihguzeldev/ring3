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
}
