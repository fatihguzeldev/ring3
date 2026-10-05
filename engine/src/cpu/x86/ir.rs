use super::Register32;
use crate::memory::GuestAddress;

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
pub enum ByteRegister {
    Al,
    Cl,
    Dl,
    Bl,
    Ah,
    Ch,
    Dh,
    Bh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteValue {
    Register(ByteRegister),
    Immediate(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytePredicateKind {
    Cmp,
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteLogicalKind {
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteArithmeticKind {
    Add,
    Adc,
    Sub,
    Sbb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteReadArithmeticKind {
    Add,
    Adc,
    Sub,
    Sbb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryByteArithmeticKind {
    Add,
    Adc,
    Sub,
    Sbb,
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
    Adc,
    Sub,
    Sbb,
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
pub enum ShiftKind {
    Shl,
    Shr,
    Sar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShiftCount {
    Immediate(u8),
    Cl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchTarget {
    Direct(GuestAddress),
    Indirect(Location32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Overflow,
    NotOverflow,
    Below,
    AboveOrEqual,
    Equal,
    NotEqual,
    BelowOrEqual,
    Above,
    Sign,
    NotSign,
    Parity,
    NotParity,
    Less,
    GreaterOrEqual,
    LessOrEqual,
    Greater,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Nop,
    Move {
        destination: Location32,
        source: Value32,
    },
    MoveByte {
        destination: ByteRegister,
        source: ByteValue,
    },
    CompareByte {
        left: ByteRegister,
        right: ByteValue,
    },
    ReadCompareByte {
        left: ByteRegister,
        address: EffectiveAddress,
    },
    TestByte {
        left: ByteRegister,
        right: ByteValue,
    },
    MemoryPredicateByte {
        kind: BytePredicateKind,
        address: EffectiveAddress,
        right: ByteValue,
    },
    MemoryLogicalByte {
        kind: ByteLogicalKind,
        address: EffectiveAddress,
        source: ByteValue,
    },
    MemoryArithmeticByte {
        kind: MemoryByteArithmeticKind,
        address: EffectiveAddress,
        source: ByteValue,
    },
    LogicalByte {
        kind: ByteLogicalKind,
        destination: ByteRegister,
        source: ByteValue,
    },
    ReadLogicalByte {
        kind: ByteLogicalKind,
        destination: ByteRegister,
        address: EffectiveAddress,
    },
    ArithmeticByte {
        kind: ByteArithmeticKind,
        destination: ByteRegister,
        source: ByteValue,
    },
    ReadArithmeticByte {
        kind: ByteReadArithmeticKind,
        destination: ByteRegister,
        address: EffectiveAddress,
    },
    SetByte {
        condition: Condition,
        destination: ByteRegister,
    },
    MemorySetByte {
        condition: Condition,
        address: EffectiveAddress,
    },
    LoadByte {
        destination: ByteRegister,
        address: EffectiveAddress,
    },
    StoreByte {
        address: EffectiveAddress,
        source: ByteValue,
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
    UnaryByte {
        kind: UnaryKind,
        destination: ByteRegister,
    },
    MemoryUnaryByte {
        kind: UnaryKind,
        address: EffectiveAddress,
    },
    Unary {
        kind: UnaryKind,
        destination: Location32,
    },
    ShiftByte {
        kind: ShiftKind,
        destination: ByteRegister,
        count: ShiftCount,
    },
    MemoryShiftByte {
        kind: ShiftKind,
        address: EffectiveAddress,
    },
    Shift {
        kind: ShiftKind,
        destination: Location32,
        count: ShiftCount,
    },
    SignedMultiply {
        destination: Register32,
        source: Location32,
        immediate: Option<u32>,
    },
    Jump {
        target: BranchTarget,
    },
    ConditionalJump {
        condition: Condition,
        target: GuestAddress,
    },
    Call {
        target: BranchTarget,
    },
    Push {
        source: Value32,
    },
    Pop {
        destination: Location32,
    },
    Leave,
    Return {
        stack_adjust: u16,
    },
}
