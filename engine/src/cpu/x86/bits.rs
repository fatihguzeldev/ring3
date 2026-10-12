use super::{
    Register32,
    ir::{ByteRegister, EffectiveAddress},
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crc32Width {
    Byte,
    Word,
    Dword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crc32Source {
    ByteRegister(ByteRegister),
    WordRegister(Register32),
    DwordRegister(Register32),
    Memory {
        width: Crc32Width,
        address: EffectiveAddress,
    },
}
