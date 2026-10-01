use super::Register32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveAddress {
    pub base: Option<Register32>,
    pub index: Option<Register32>,
    pub scale: u8,
    pub displacement: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location32 {
    Register(Register32),
    Memory(EffectiveAddress),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value32 {
    Register(Register32),
    Memory(EffectiveAddress),
    Immediate(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallWidth {
    Byte,
    Word,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallSource {
    Register {
        register: Register32,
        width: SmallWidth,
        high_byte: bool,
    },
    Memory {
        address: EffectiveAddress,
        width: SmallWidth,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionKind {
    Zero,
    Sign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryKind {
    Add,
    Sub,
    Cmp,
    Test,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryKind {
    Inc,
    Dec,
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Nop,
    Move {
        destination: Location32,
        source: Value32,
    },
    Extend {
        kind: ExtensionKind,
        destination: Register32,
        source: SmallSource,
    },
    Lea {
        destination: Register32,
        address: EffectiveAddress,
    },
    Binary {
        kind: BinaryKind,
        destination: Location32,
        source: Value32,
    },
    Unary {
        kind: UnaryKind,
        destination: Location32,
    },
}
