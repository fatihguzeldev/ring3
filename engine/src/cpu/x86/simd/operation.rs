use crate::cpu::x86::{Register32, ir::EffectiveAddress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdRegister {
    Mmx(u8),
    Xmm(u8),
    Gpr(Register32),
}

impl SimdRegister {
    pub fn bytes(self) -> u8 {
        match self {
            Self::Mmx(_) => 8,
            Self::Xmm(_) => 16,
            Self::Gpr(_) => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdOperand {
    Register(SimdRegister),
    Memory(EffectiveAddress),
    None,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedKind {
    Add,
    Subtract,
    AddSignedSaturate,
    AddUnsignedSaturate,
    SubtractSignedSaturate,
    SubtractUnsignedSaturate,
    And,
    AndNot,
    Or,
    Xor,
    Equal,
    GreaterSigned,
    MultiplyLow,
    MultiplyHighSigned,
    MultiplyHighUnsigned,
    MultiplyUnsignedDword,
    MultiplyAddWords,
    AverageUnsigned,
    MinimumSigned,
    MaximumSigned,
    MinimumUnsigned,
    MaximumUnsigned,
    SumAbsoluteByteDifferences,
    ShiftLeft,
    ShiftRight,
    ShiftRightSigned,
    Absolute,
    Sign,
    HorizontalAdd,
    HorizontalSubtract,
    HorizontalAddSignedSaturate,
    HorizontalSubtractSignedSaturate,
    MultiplyAddUnsignedSignedBytes,
    MultiplyHighRoundSigned,
    MultiplySignedDword,
    MultipleSumAbsoluteByteDifferences,
    MinimumPositionUnsignedWord,
    Test,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShuffleKind {
    Dwords,
    Words,
    HighWords,
    LowWords,
    Singles,
    Doubles,
    UnpackLow,
    UnpackHigh,
    PackSigned,
    PackUnsigned,
    ShiftBytesLeft,
    ShiftBytesRight,
    InsertWord,
    ExtractWord,
    MaskBytes,
    MaskSingles,
    MaskDoubles,
    MoveLowToHigh,
    MoveHighToLow,
    Bytes,
    AlignRight,
    DuplicateEven,
    DuplicateOdd,
    SignExtendBytes,
    SignExtendWords,
    SignExtendDwords,
    ZeroExtendBytes,
    ZeroExtendWords,
    ZeroExtendDwords,
    BlendImmediate,
    BlendVariable,
    InsertByte,
    InsertDword,
    InsertSingle,
    ExtractByte,
    ExtractDword,
}

impl ShuffleKind {
    pub(crate) fn widening(self) -> Option<(u8, bool)> {
        match self {
            Self::SignExtendBytes => Some((1, true)),
            Self::SignExtendWords => Some((2, true)),
            Self::SignExtendDwords => Some((4, true)),
            Self::ZeroExtendBytes => Some((1, false)),
            Self::ZeroExtendWords => Some((2, false)),
            Self::ZeroExtendDwords => Some((4, false)),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatKind {
    Add,
    Subtract,
    Multiply,
    Divide,
    SquareRoot,
    Minimum,
    Maximum,
    Compare,
    CompareFlags,
    CompareFlagsQuiet,
    Reciprocal,
    ReciprocalSquareRoot,
    AddSubtract,
    HorizontalAdd,
    HorizontalSubtract,
    Round,
    DotProduct,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvertKind {
    SignedDwordsToSingles,
    SignedDwordsToDoubles,
    SinglesToSignedDwords,
    DoublesToSignedDwords,
    SinglesToDoubles,
    DoublesToSingles,
    SignedDwordToSingle,
    SignedDwordToDouble,
    SingleToSignedDword,
    DoubleToSignedDword,
    SingleToDouble,
    DoubleToSingle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CryptoKind {
    EncryptRound,
    EncryptLast,
    DecryptRound,
    DecryptLast,
    InverseMixColumns,
    KeyAssist,
    CarrylessMultiply,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdKind {
    Move,
    MoveScalar,
    MoveLow,
    MoveHigh,
    Packed(PackedKind),
    Shuffle(ShuffleKind),
    Float(FloatKind),
    Convert { kind: ConvertKind, truncate: bool },
    Crypto(CryptoKind),
    LoadMxcsr,
    StoreMxcsr,
    EmptyMmx,
    SaveExtended,
    RestoreExtended,
    MaskedStore,
    Fence,
    Prefetch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimdOperation {
    pub kind: SimdKind,
    pub destination: SimdOperand,
    pub source: SimdOperand,
    pub vector_bytes: u8,
    pub lane_bytes: u8,
    pub memory_bytes: u16,
    pub immediate: Option<u8>,
    pub aligned: bool,
    pub scalar: bool,
    pub implicit_address: Option<EffectiveAddress>,
}

impl SimdOperation {
    pub fn reads_memory(self) -> bool {
        self.kind != SimdKind::Prefetch && matches!(self.source, SimdOperand::Memory(_))
            || matches!(self.kind, SimdKind::LoadMxcsr | SimdKind::RestoreExtended)
    }

    pub fn writes_memory(self) -> bool {
        matches!(self.destination, SimdOperand::Memory(_))
            || matches!(
                self.kind,
                SimdKind::StoreMxcsr | SimdKind::SaveExtended | SimdKind::MaskedStore
            )
    }

    pub fn uses_mmx(self) -> bool {
        matches!(
            self.destination,
            SimdOperand::Register(SimdRegister::Mmx(_))
        ) || matches!(self.source, SimdOperand::Register(SimdRegister::Mmx(_)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdError {
    InvalidDescriptor,
    GeneralProtection(u32),
    FloatingPoint(u32),
    X87FloatingPoint(u16),
    InvalidState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimdResult {
    pub gpr: u32,
    pub flags: u32,
}
