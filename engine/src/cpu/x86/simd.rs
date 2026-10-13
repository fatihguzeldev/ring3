pub mod ymm;

use super::{Register32, ir::EffectiveAddress};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimdState {
    pub mxcsr: u32,
    pub xmm: [[u8; 16]; 8],
}

impl Default for SimdState {
    fn default() -> Self {
        Self {
            mxcsr: 0x1f80,
            xmm: [[0; 16]; 8],
        }
    }
}

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

mod crypto;
mod float;
mod packed;

pub(crate) use float::{ElementwiseKind, elementwise};

mod integer;
pub(crate) use integer::IntegerLaneOperation;

pub(crate) use float::{CompareMode, compare_lanes, conversion_lanes};

pub(crate) use float::{PairwiseKind, dot_product_lanes, pairwise_lanes, round_lanes};

mod rearrange;
pub(crate) use rearrange::RearrangeOperation;

mod shift;
pub(crate) use shift::ShiftOperation;

mod multiply;
pub(crate) use multiply::MultiplyOperation;

mod scalar;
pub(crate) use scalar::{ScalarOperation, ScalarWidth};

mod predicates;
pub(crate) use predicates::BitTestKind;
pub(crate) use predicates::flags as test_flags;

pub(crate) use crypto::execute as crypto_value;

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

fn register_number(register: Register32) -> u32 {
    register.index() as u32
}
fn operand_code(operand: SimdOperand) -> u32 {
    match operand {
        SimdOperand::Register(SimdRegister::Mmx(index)) => u32::from(index),
        SimdOperand::Register(SimdRegister::Xmm(index)) => 8 + u32::from(index),
        SimdOperand::Register(SimdRegister::Gpr(register)) => 16 + register_number(register),
        SimdOperand::Memory(_) => 24,
        SimdOperand::None => 25,
    }
}
fn operand_from_code(code: u32) -> Result<SimdOperand, SimdError> {
    Ok(match code {
        0..=7 => SimdOperand::Register(SimdRegister::Mmx(code as u8)),
        8..=15 => SimdOperand::Register(SimdRegister::Xmm((code - 8) as u8)),
        16..=23 => SimdOperand::Register(SimdRegister::Gpr(
            [
                Register32::Eax,
                Register32::Ecx,
                Register32::Edx,
                Register32::Ebx,
                Register32::Esp,
                Register32::Ebp,
                Register32::Esi,
                Register32::Edi,
            ][(code - 16) as usize],
        )),
        24 => SimdOperand::Memory(EffectiveAddress::default()),
        25 => SimdOperand::None,
        _ => return Err(SimdError::InvalidDescriptor),
    })
}

impl SimdOperation {
    pub fn descriptor(self) -> (u32, u32, u32) {
        let (kind, sub, truncate) = match self.kind {
            SimdKind::Move => (0, 0, false),
            SimdKind::MoveScalar => (1, 0, false),
            SimdKind::MoveLow => (2, 0, false),
            SimdKind::MoveHigh => (3, 0, false),
            SimdKind::Packed(kind) => (4, kind as u32, false),
            SimdKind::Shuffle(kind) => (5, kind as u32, false),
            SimdKind::Float(kind) => (6, kind as u32, false),
            SimdKind::Convert { kind, truncate } => (7, kind as u32, truncate),
            SimdKind::Crypto(kind) => (16, kind as u32, false),
            SimdKind::LoadMxcsr => (8, 0, false),
            SimdKind::StoreMxcsr => (9, 0, false),
            SimdKind::EmptyMmx => (10, 0, false),
            SimdKind::SaveExtended => (11, 0, false),
            SimdKind::RestoreExtended => (12, 0, false),
            SimdKind::MaskedStore => (13, 0, false),
            SimdKind::Fence => (14, 0, false),
            SimdKind::Prefetch => (15, 0, false),
        };
        let lane = [0, 1, 2, 4, 8, 16]
            .iter()
            .position(|width| *width == self.lane_bytes)
            .unwrap() as u32;
        let descriptor = (kind & 15)
            | ((kind & 16) << 20)
            | (sub << 4)
            | (lane << 10)
            | (u32::from(self.vector_bytes == 16) << 13)
            | (u32::from(self.vector_bytes == 4) << 14)
            | (([0, 1, 2, 4, 8, 16, 512]
                .iter()
                .position(|width| *width == self.memory_bytes)
                .unwrap() as u32)
                << 15)
            | (u32::from(self.aligned) << 20)
            | (u32::from(self.scalar) << 21)
            | (u32::from(self.immediate.is_some()) << 22)
            | (u32::from(truncate) << 23);
        (
            descriptor,
            operand_code(self.destination) | (operand_code(self.source) << 8),
            u32::from(self.immediate.unwrap_or(0)),
        )
    }
}

pub fn decode_descriptor(
    descriptor: u32,
    operands: u32,
    immediate: u32,
) -> Result<SimdOperation, SimdError> {
    use ConvertKind as C;
    use CryptoKind as A;
    use FloatKind as F;
    use PackedKind as P;
    use ShuffleKind as S;
    if descriptor & 0xfe00_0000 != 0 || operands & !0x3f3f != 0 || immediate > 255 {
        return Err(SimdError::InvalidDescriptor);
    }
    let sub = ((descriptor >> 4) & 63) as usize;
    let plain = |kind| {
        if sub == 0 {
            Ok(kind)
        } else {
            Err(SimdError::InvalidDescriptor)
        }
    };
    let kind = match (descriptor & 15) | ((descriptor >> 20) & 16) {
        0 => plain(SimdKind::Move)?,
        1 => plain(SimdKind::MoveScalar)?,
        2 => plain(SimdKind::MoveLow)?,
        3 => plain(SimdKind::MoveHigh)?,
        4 => SimdKind::Packed(
            *[
                P::Add,
                P::Subtract,
                P::AddSignedSaturate,
                P::AddUnsignedSaturate,
                P::SubtractSignedSaturate,
                P::SubtractUnsignedSaturate,
                P::And,
                P::AndNot,
                P::Or,
                P::Xor,
                P::Equal,
                P::GreaterSigned,
                P::MultiplyLow,
                P::MultiplyHighSigned,
                P::MultiplyHighUnsigned,
                P::MultiplyUnsignedDword,
                P::MultiplyAddWords,
                P::AverageUnsigned,
                P::MinimumSigned,
                P::MaximumSigned,
                P::MinimumUnsigned,
                P::MaximumUnsigned,
                P::SumAbsoluteByteDifferences,
                P::ShiftLeft,
                P::ShiftRight,
                P::ShiftRightSigned,
                P::Absolute,
                P::Sign,
                P::HorizontalAdd,
                P::HorizontalSubtract,
                P::HorizontalAddSignedSaturate,
                P::HorizontalSubtractSignedSaturate,
                P::MultiplyAddUnsignedSignedBytes,
                P::MultiplyHighRoundSigned,
                P::MultiplySignedDword,
                P::MultipleSumAbsoluteByteDifferences,
                P::MinimumPositionUnsignedWord,
                P::Test,
            ]
            .get(sub)
            .ok_or(SimdError::InvalidDescriptor)?,
        ),
        5 => SimdKind::Shuffle(
            *[
                S::Dwords,
                S::Words,
                S::HighWords,
                S::LowWords,
                S::Singles,
                S::Doubles,
                S::UnpackLow,
                S::UnpackHigh,
                S::PackSigned,
                S::PackUnsigned,
                S::ShiftBytesLeft,
                S::ShiftBytesRight,
                S::InsertWord,
                S::ExtractWord,
                S::MaskBytes,
                S::MaskSingles,
                S::MaskDoubles,
                S::MoveLowToHigh,
                S::MoveHighToLow,
                S::Bytes,
                S::AlignRight,
                S::DuplicateEven,
                S::DuplicateOdd,
                S::SignExtendBytes,
                S::SignExtendWords,
                S::SignExtendDwords,
                S::ZeroExtendBytes,
                S::ZeroExtendWords,
                S::ZeroExtendDwords,
                S::BlendImmediate,
                S::BlendVariable,
                S::InsertByte,
                S::InsertDword,
                S::InsertSingle,
                S::ExtractByte,
                S::ExtractDword,
            ]
            .get(sub)
            .ok_or(SimdError::InvalidDescriptor)?,
        ),
        6 => SimdKind::Float(
            *[
                F::Add,
                F::Subtract,
                F::Multiply,
                F::Divide,
                F::SquareRoot,
                F::Minimum,
                F::Maximum,
                F::Compare,
                F::CompareFlags,
                F::CompareFlagsQuiet,
                F::Reciprocal,
                F::ReciprocalSquareRoot,
                F::AddSubtract,
                F::HorizontalAdd,
                F::HorizontalSubtract,
                F::Round,
                F::DotProduct,
            ]
            .get(sub)
            .ok_or(SimdError::InvalidDescriptor)?,
        ),
        7 => SimdKind::Convert {
            kind: *[
                C::SignedDwordsToSingles,
                C::SignedDwordsToDoubles,
                C::SinglesToSignedDwords,
                C::DoublesToSignedDwords,
                C::SinglesToDoubles,
                C::DoublesToSingles,
                C::SignedDwordToSingle,
                C::SignedDwordToDouble,
                C::SingleToSignedDword,
                C::DoubleToSignedDword,
                C::SingleToDouble,
                C::DoubleToSingle,
            ]
            .get(sub)
            .ok_or(SimdError::InvalidDescriptor)?,
            truncate: descriptor & (1 << 23) != 0,
        },
        8 => plain(SimdKind::LoadMxcsr)?,
        9 => plain(SimdKind::StoreMxcsr)?,
        10 => plain(SimdKind::EmptyMmx)?,
        11 => plain(SimdKind::SaveExtended)?,
        12 => plain(SimdKind::RestoreExtended)?,
        13 => plain(SimdKind::MaskedStore)?,
        14 => plain(SimdKind::Fence)?,
        15 => plain(SimdKind::Prefetch)?,
        16 => SimdKind::Crypto(
            *[
                A::EncryptRound,
                A::EncryptLast,
                A::DecryptRound,
                A::DecryptLast,
                A::InverseMixColumns,
                A::KeyAssist,
                A::CarrylessMultiply,
            ]
            .get(sub)
            .ok_or(SimdError::InvalidDescriptor)?,
        ),
        _ => return Err(SimdError::InvalidDescriptor),
    };
    let lane_bytes = *[0, 1, 2, 4, 8, 16]
        .get(((descriptor >> 10) & 7) as usize)
        .ok_or(SimdError::InvalidDescriptor)?;
    let memory_bytes = *[0u16, 1, 2, 4, 8, 16, 512]
        .get(((descriptor >> 15) & 31) as usize)
        .ok_or(SimdError::InvalidDescriptor)?;
    let operation = SimdOperation {
        kind,
        destination: operand_from_code(operands & 63)?,
        source: operand_from_code((operands >> 8) & 63)?,
        vector_bytes: if descriptor & (1 << 14) != 0 {
            4
        } else if descriptor & (1 << 13) != 0 {
            16
        } else {
            8
        },
        lane_bytes,
        memory_bytes,
        immediate: (descriptor & (1 << 22) != 0).then_some(immediate as u8),
        aligned: descriptor & (1 << 20) != 0,
        scalar: descriptor & (1 << 21) != 0,
        implicit_address: (kind == SimdKind::MaskedStore).then(|| EffectiveAddress {
            base: Some(Register32::Edi),
            ..Default::default()
        }),
    };
    if memory_bytes > 512
        || descriptor & (1 << 13) != 0 && descriptor & (1 << 14) != 0
        || descriptor & (1 << 23) != 0 && !matches!(kind, SimdKind::Convert { .. })
        || operation.immediate.is_none() && immediate != 0
    {
        return Err(SimdError::InvalidDescriptor);
    }
    validate_operation(operation)?;
    Ok(operation)
}

fn validate_operation(op: SimdOperation) -> Result<(), SimdError> {
    use SimdKind as K;
    use SimdOperand as O;
    let invalid = || Err(SimdError::InvalidDescriptor);
    let xmm = |operand| matches!(operand, O::Register(SimdRegister::Xmm(0..=7)));
    let mmx = |operand| matches!(operand, O::Register(SimdRegister::Mmx(0..=7)));
    let gpr = |operand| matches!(operand, O::Register(SimdRegister::Gpr(_)));
    let memory = |operand| matches!(operand, O::Memory(_));
    let vector = |operand| xmm(operand) || mmx(operand);
    let same = |a, b| xmm(a) && xmm(b) || mmx(a) && mmx(b);
    if [op.destination, op.source].into_iter().any(|operand| {
        matches!(
            operand,
            O::Register(SimdRegister::Mmx(8..) | SimdRegister::Xmm(8..))
        )
    }) {
        return invalid();
    }
    if op.kind != K::MaskedStore && op.implicit_address.is_some() {
        return invalid();
    }
    if !memory(op.destination)
        && !memory(op.source)
        && op.kind != K::MaskedStore
        && op.memory_bytes != 0
    {
        return invalid();
    }
    if op.scalar && !matches!(op.kind, K::MoveScalar | K::Float(_) | K::Convert { .. }) {
        return invalid();
    }
    if let K::Crypto(kind) = op.kind {
        return if op.vector_bytes == 16
            && op.lane_bytes
                == if kind == CryptoKind::CarrylessMultiply {
                    8
                } else {
                    1
                }
            && xmm(op.destination)
            && (xmm(op.source) || memory(op.source))
            && op.memory_bytes == if memory(op.source) { 16 } else { 0 }
            && op.aligned == memory(op.source)
            && op.immediate.is_some()
                == matches!(kind, CryptoKind::KeyAssist | CryptoKind::CarrylessMultiply)
        {
            Ok(())
        } else {
            invalid()
        };
    }
    if matches!(op.kind, K::Fence | K::Prefetch) {
        return if op.destination == O::None
            && op.vector_bytes == 4
            && op.lane_bytes == 0
            && !op.scalar
            && !op.aligned
            && op.immediate.is_none()
            && if op.kind == K::Fence {
                op.source == O::None
            } else {
                memory(op.source)
            } {
            Ok(())
        } else {
            invalid()
        };
    }
    if op.kind == K::MaskedStore {
        return if op.memory_bytes == u16::from(op.vector_bytes)
            && matches!(op.vector_bytes, 8 | 16)
            && op.lane_bytes == 1
            && !op.scalar
            && !op.aligned
            && op.immediate.is_none()
            && op.implicit_address.is_some()
            && same(op.destination, op.source)
            && if op.vector_bytes == 8 {
                mmx(op.destination)
            } else {
                xmm(op.destination)
            } {
            Ok(())
        } else {
            invalid()
        };
    }
    if matches!(op.destination, O::Memory(_)) && matches!(op.source, O::Memory(_)) {
        return invalid();
    }
    if matches!(op.kind, K::EmptyMmx) {
        return if op.destination == O::None
            && op.source == O::None
            && op.vector_bytes == 4
            && op.lane_bytes == 0
            && !op.aligned
            && !op.scalar
            && op.immediate.is_none()
        {
            Ok(())
        } else {
            invalid()
        };
    }
    if matches!(op.kind, K::SaveExtended | K::RestoreExtended) {
        return if op.memory_bytes == 512
            && op.aligned
            && op.vector_bytes == 4
            && op.lane_bytes == 0
            && !op.scalar
            && op.immediate.is_none()
            && if op.kind == K::SaveExtended {
                memory(op.destination) && op.source == O::None
            } else {
                memory(op.source) && op.destination == O::None
            } {
            Ok(())
        } else {
            invalid()
        };
    }
    if matches!(op.kind, K::LoadMxcsr | K::StoreMxcsr) {
        return if op.vector_bytes == 4
            && op.lane_bytes == 4
            && op.memory_bytes == 4
            && !op.aligned
            && !op.scalar
            && op.immediate.is_none()
            && if op.kind == K::LoadMxcsr {
                memory(op.source) && op.destination == O::None
            } else {
                memory(op.destination) && op.source == O::None
            } {
            Ok(())
        } else {
            invalid()
        };
    }
    if !matches!(op.vector_bytes, 8 | 16) || !matches!(op.lane_bytes, 1 | 2 | 4 | 8 | 16) {
        return invalid();
    }
    let register_bytes = [op.destination, op.source]
        .into_iter()
        .filter_map(|operand| match operand {
            O::Register(register) => Some(register.bytes()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    if op.vector_bytes != register_bytes
        && !(op.kind == K::Move
            && memory(op.destination)
            && gpr(op.source)
            && op.vector_bytes == 8
            && op.lane_bytes == 4)
    {
        return invalid();
    }
    if matches!(op.destination, O::None) {
        return invalid();
    }
    if matches!(op.kind, K::Float(_) | K::Packed(_))
        && !matches!(
            op.destination,
            O::Register(SimdRegister::Xmm(_) | SimdRegister::Mmx(_))
        )
    {
        return invalid();
    }
    if matches!(op.kind, K::Float(_) | K::Convert { .. }) && op.vector_bytes != 16 && !op.uses_mmx()
    {
        return invalid();
    }
    if matches!(op.source, O::None)
        && !matches!(
            op.kind,
            K::Packed(
                PackedKind::ShiftLeft | PackedKind::ShiftRight | PackedKind::ShiftRightSigned
            ) | K::Shuffle(ShuffleKind::ShiftBytesLeft | ShuffleKind::ShiftBytesRight)
        )
    {
        return invalid();
    }
    if op.reads_memory() || op.writes_memory() {
        let expected = match op.kind {
            K::Move | K::MoveScalar => op.lane_bytes,
            K::MoveLow | K::MoveHigh => 8,
            K::Packed(_) => op.vector_bytes,
            K::Shuffle(kind) if kind.widening().is_some() => {
                kind.widening().unwrap().0 * 16 / op.lane_bytes
            }
            K::Shuffle(
                ShuffleKind::InsertByte
                | ShuffleKind::InsertWord
                | ShuffleKind::InsertDword
                | ShuffleKind::InsertSingle
                | ShuffleKind::ExtractByte
                | ShuffleKind::ExtractWord
                | ShuffleKind::ExtractDword,
            ) => op.lane_bytes,
            K::Shuffle(ShuffleKind::DuplicateEven) if op.lane_bytes == 8 => 8,
            K::Shuffle(
                ShuffleKind::MaskBytes | ShuffleKind::MaskSingles | ShuffleKind::MaskDoubles,
            ) => return invalid(),
            K::Shuffle(_) => op.vector_bytes,
            K::Float(_) => {
                if op.scalar {
                    op.lane_bytes
                } else {
                    16
                }
            }
            K::Convert { kind, .. } => match kind {
                ConvertKind::SignedDwordsToSingles => {
                    if op.uses_mmx() || op.memory_bytes == 8 {
                        8
                    } else {
                        16
                    }
                }
                ConvertKind::SignedDwordsToDoubles | ConvertKind::SinglesToDoubles => 8,
                ConvertKind::SinglesToSignedDwords => {
                    if op.uses_mmx() {
                        8
                    } else {
                        16
                    }
                }
                ConvertKind::DoublesToSignedDwords | ConvertKind::DoublesToSingles => 16,
                ConvertKind::SignedDwordToSingle
                | ConvertKind::SignedDwordToDouble
                | ConvertKind::SingleToSignedDword
                | ConvertKind::SingleToDouble => 4,
                ConvertKind::DoubleToSignedDword | ConvertKind::DoubleToSingle => 8,
            },
            _ => return invalid(),
        };
        if op.memory_bytes != u16::from(expected) {
            return invalid();
        }
    }
    if op.writes_memory()
        && !matches!(
            op.kind,
            K::Move
                | K::MoveScalar
                | K::MoveLow
                | K::MoveHigh
                | K::Shuffle(
                    ShuffleKind::ExtractByte | ShuffleKind::ExtractWord | ShuffleKind::ExtractDword
                )
        )
    {
        return invalid();
    }
    match op.kind {
        K::Move => {
            if op.immediate.is_some() || op.scalar || !matches!(op.lane_bytes, 4 | 8 | 16) {
                return invalid();
            }
            if op.lane_bytes == 16 && !(xmm(op.destination) || memory(op.destination)) {
                return invalid();
            }
            if !(vector(op.destination) || gpr(op.destination) || memory(op.destination))
                || !(vector(op.source) || gpr(op.source) || memory(op.source))
            {
                return invalid();
            }
            if (gpr(op.destination) || gpr(op.source)) && op.lane_bytes != 4 {
                return invalid();
            }
            if !vector(op.destination)
                && !vector(op.source)
                && !(memory(op.destination) && gpr(op.source))
            {
                return invalid();
            }
        }
        K::MoveScalar => {
            if op.vector_bytes != 16
                || !matches!(op.lane_bytes, 4 | 8)
                || !op.scalar
                || op.immediate.is_some()
                || !(xmm(op.destination) || memory(op.destination))
                || !(xmm(op.source) || memory(op.source))
            {
                return invalid();
            }
        }
        K::MoveLow | K::MoveHigh => {
            if op.vector_bytes != 16
                || op.lane_bytes != 8
                || op.immediate.is_some()
                || !((xmm(op.destination) && memory(op.source))
                    || (memory(op.destination) && xmm(op.source)))
            {
                return invalid();
            }
        }
        K::Packed(kind) => {
            let sad = kind == PackedKind::MultipleSumAbsoluteByteDifferences;
            let shift = matches!(
                kind,
                PackedKind::ShiftLeft | PackedKind::ShiftRight | PackedKind::ShiftRightSigned
            );
            if !vector(op.destination)
                || !(same(op.destination, op.source)
                    || memory(op.source)
                    || shift && op.source == O::None && op.immediate.is_some())
                || (!shift && !sad && op.immediate.is_some())
                || (sad && op.immediate.is_none())
                || (shift && op.immediate.is_some() && op.source != O::None)
            {
                return invalid();
            }
            if op.lane_bytes > 8 {
                return invalid();
            }
            if matches!(
                kind,
                PackedKind::MultiplyHighSigned
                    | PackedKind::MultiplyHighUnsigned
                    | PackedKind::MultiplyAddWords
            ) && op.lane_bytes != 2
            {
                return invalid();
            }
            if matches!(kind, PackedKind::Absolute | PackedKind::Sign) && op.lane_bytes > 4
                || matches!(
                    kind,
                    PackedKind::HorizontalAdd | PackedKind::HorizontalSubtract
                ) && !matches!(op.lane_bytes, 2 | 4)
                || matches!(
                    kind,
                    PackedKind::HorizontalAddSignedSaturate
                        | PackedKind::HorizontalSubtractSignedSaturate
                        | PackedKind::MultiplyHighRoundSigned
                ) && op.lane_bytes != 2
                || kind == PackedKind::MultiplyAddUnsignedSignedBytes && op.lane_bytes != 1
            {
                return invalid();
            }
            if kind == PackedKind::MultiplyLow
                && (!matches!(op.lane_bytes, 2 | 4) || op.lane_bytes == 4 && op.vector_bytes != 16)
                || kind == PackedKind::MultiplySignedDword
                    && (op.lane_bytes != 4 || op.vector_bytes != 16)
            {
                return invalid();
            }
            if matches!(
                kind,
                PackedKind::MultipleSumAbsoluteByteDifferences
                    | PackedKind::MinimumPositionUnsignedWord
                    | PackedKind::Test
            ) && (op.vector_bytes != 16
                || op.lane_bytes
                    != match kind {
                        PackedKind::MultipleSumAbsoluteByteDifferences => 1,
                        PackedKind::MinimumPositionUnsignedWord => 2,
                        PackedKind::Test => 8,
                        _ => unreachable!(),
                    })
            {
                return invalid();
            }
            if matches!(kind, PackedKind::MultiplyUnsignedDword) && op.lane_bytes != 4 {
                return invalid();
            }
            if matches!(
                kind,
                PackedKind::AverageUnsigned
                    | PackedKind::AddSignedSaturate
                    | PackedKind::AddUnsignedSaturate
                    | PackedKind::SubtractSignedSaturate
                    | PackedKind::SubtractUnsignedSaturate
            ) && op.lane_bytes > 2
            {
                return invalid();
            }
            if matches!(
                kind,
                PackedKind::And | PackedKind::AndNot | PackedKind::Or | PackedKind::Xor
            ) && op.lane_bytes != 8
                || matches!(kind, PackedKind::Equal | PackedKind::GreaterSigned)
                    && op.lane_bytes == 8
                    && op.vector_bytes != 16
                || matches!(kind, PackedKind::MinimumSigned | PackedKind::MaximumSigned)
                    && (!matches!(op.lane_bytes, 1 | 2 | 4)
                        || op.lane_bytes != 2 && op.vector_bytes != 16)
                || matches!(
                    kind,
                    PackedKind::MinimumUnsigned | PackedKind::MaximumUnsigned
                ) && (!matches!(op.lane_bytes, 1 | 2 | 4)
                    || op.lane_bytes != 1 && op.vector_bytes != 16)
                || kind == PackedKind::SumAbsoluteByteDifferences && op.lane_bytes != 1
                || shift && op.lane_bytes < 2
                || kind == PackedKind::ShiftRightSigned && op.lane_bytes == 8
            {
                return invalid();
            }
        }
        K::Shuffle(kind) => {
            let immediate = matches!(
                kind,
                ShuffleKind::Dwords
                    | ShuffleKind::Words
                    | ShuffleKind::HighWords
                    | ShuffleKind::LowWords
                    | ShuffleKind::Singles
                    | ShuffleKind::Doubles
                    | ShuffleKind::ShiftBytesLeft
                    | ShuffleKind::ShiftBytesRight
                    | ShuffleKind::InsertWord
                    | ShuffleKind::ExtractWord
                    | ShuffleKind::AlignRight
                    | ShuffleKind::BlendImmediate
                    | ShuffleKind::InsertByte
                    | ShuffleKind::InsertDword
                    | ShuffleKind::InsertSingle
                    | ShuffleKind::ExtractByte
                    | ShuffleKind::ExtractDword
            );
            if op.immediate.is_some() != immediate {
                return invalid();
            }
            let operands = match kind {
                ShuffleKind::ExtractWord => {
                    gpr(op.destination) && vector(op.source)
                        || memory(op.destination) && xmm(op.source)
                }
                ShuffleKind::ExtractByte | ShuffleKind::ExtractDword => {
                    (gpr(op.destination) || memory(op.destination)) && xmm(op.source)
                }
                ShuffleKind::MaskBytes => gpr(op.destination) && vector(op.source),
                ShuffleKind::MaskSingles | ShuffleKind::MaskDoubles => {
                    gpr(op.destination) && xmm(op.source)
                }
                ShuffleKind::InsertWord => {
                    vector(op.destination) && (gpr(op.source) || memory(op.source))
                }
                ShuffleKind::InsertByte | ShuffleKind::InsertDword => {
                    xmm(op.destination) && (gpr(op.source) || memory(op.source))
                }
                ShuffleKind::ShiftBytesLeft | ShuffleKind::ShiftBytesRight => {
                    xmm(op.destination) && op.source == O::None
                }
                _ => {
                    vector(op.destination) && (same(op.destination, op.source) || memory(op.source))
                }
            };
            if !operands {
                return invalid();
            }
            if let Some((from, _)) = kind.widening()
                && (op.vector_bytes != 16 || op.lane_bytes <= from || op.lane_bytes > 8)
            {
                return invalid();
            }
            if matches!(
                kind,
                ShuffleKind::BlendImmediate | ShuffleKind::BlendVariable
            ) && (op.vector_bytes != 16
                || if kind == ShuffleKind::BlendImmediate {
                    !matches!(op.lane_bytes, 2 | 4 | 8)
                } else {
                    !matches!(op.lane_bytes, 1 | 4 | 8)
                })
                || matches!(kind, ShuffleKind::InsertByte | ShuffleKind::ExtractByte)
                    && (op.vector_bytes != 16 || op.lane_bytes != 1)
                || matches!(
                    kind,
                    ShuffleKind::InsertDword
                        | ShuffleKind::InsertSingle
                        | ShuffleKind::ExtractDword
                ) && (op.vector_bytes != 16 || op.lane_bytes != 4)
            {
                return invalid();
            }
            if matches!(kind, ShuffleKind::DuplicateEven | ShuffleKind::DuplicateOdd)
                && (op.vector_bytes != 16
                    || !xmm(op.destination)
                    || !(xmm(op.source) || memory(op.source))
                    || !matches!(op.lane_bytes, 4 | 8)
                    || kind == ShuffleKind::DuplicateOdd && op.lane_bytes != 4)
            {
                return invalid();
            }
            if matches!(kind, ShuffleKind::Bytes | ShuffleKind::AlignRight) && op.lane_bytes != 1 {
                return invalid();
            }
            if matches!(kind, ShuffleKind::PackSigned | ShuffleKind::PackUnsigned)
                && (op.lane_bytes > 2
                    || kind == ShuffleKind::PackUnsigned
                        && op.lane_bytes == 2
                        && op.vector_bytes != 16)
            {
                return invalid();
            }
            if matches!(kind, ShuffleKind::Dwords | ShuffleKind::Singles) && op.lane_bytes != 4 {
                return invalid();
            }
            if matches!(
                kind,
                ShuffleKind::Words
                    | ShuffleKind::HighWords
                    | ShuffleKind::LowWords
                    | ShuffleKind::InsertWord
                    | ShuffleKind::ExtractWord
            ) && op.lane_bytes != 2
            {
                return invalid();
            }
            if matches!(
                kind,
                ShuffleKind::Doubles | ShuffleKind::MoveLowToHigh | ShuffleKind::MoveHighToLow
            ) && op.vector_bytes != 16
            {
                return invalid();
            }
            if kind == ShuffleKind::Words && op.vector_bytes != 8
                || matches!(
                    kind,
                    ShuffleKind::Dwords
                        | ShuffleKind::HighWords
                        | ShuffleKind::LowWords
                        | ShuffleKind::Singles
                ) && op.vector_bytes != 16
            {
                return invalid();
            }
        }
        K::Float(kind) => {
            if !matches!(op.lane_bytes, 4 | 8)
                || op.vector_bytes != 16
                || !xmm(op.destination)
                || !(xmm(op.source) || memory(op.source))
                || matches!(
                    kind,
                    FloatKind::AddSubtract
                        | FloatKind::HorizontalAdd
                        | FloatKind::HorizontalSubtract
                        | FloatKind::DotProduct
                ) && op.scalar
                || op.immediate.is_some()
                    != matches!(
                        kind,
                        FloatKind::Compare | FloatKind::Round | FloatKind::DotProduct
                    )
                || matches!(kind, FloatKind::CompareFlags | FloatKind::CompareFlagsQuiet)
                    && !op.scalar
                || matches!(
                    kind,
                    FloatKind::Reciprocal | FloatKind::ReciprocalSquareRoot
                ) && op.lane_bytes != 4
            {
                return invalid();
            }
        }
        K::Convert { kind, truncate } => {
            use ConvertKind as C;
            let scalar = matches!(
                kind,
                C::SignedDwordToSingle
                    | C::SignedDwordToDouble
                    | C::SingleToSignedDword
                    | C::DoubleToSignedDword
                    | C::SingleToDouble
                    | C::DoubleToSingle
            );
            let to_gpr = matches!(kind, C::SingleToSignedDword | C::DoubleToSignedDword);
            let from_gpr = matches!(kind, C::SignedDwordToSingle | C::SignedDwordToDouble);
            let to_integer = matches!(
                kind,
                C::SinglesToSignedDwords
                    | C::DoublesToSignedDwords
                    | C::SingleToSignedDword
                    | C::DoubleToSignedDword
            );
            let expected_lane = if matches!(
                kind,
                C::SignedDwordsToDoubles
                    | C::DoublesToSignedDwords
                    | C::SinglesToDoubles
                    | C::SignedDwordToDouble
                    | C::DoubleToSignedDword
                    | C::SingleToDouble
            ) {
                8
            } else {
                4
            };
            if !(op.vector_bytes == 16
                || op.vector_bytes == 8 && mmx(op.destination) && memory(op.source))
                || op.lane_bytes != expected_lane
                || op.scalar != scalar
                || op.immediate.is_some()
                || truncate && !to_integer
            {
                return invalid();
            }
            if if to_gpr {
                !gpr(op.destination)
            } else {
                !(xmm(op.destination) || !scalar && to_integer && mmx(op.destination))
            } {
                return invalid();
            }
            if !(memory(op.source)
                || if from_gpr {
                    gpr(op.source)
                } else {
                    xmm(op.source)
                        || matches!(kind, C::SignedDwordsToSingles | C::SignedDwordsToDoubles)
                            && mmx(op.source)
                })
            {
                return invalid();
            }
        }
        _ => {}
    }
    Ok(())
}

fn read_operand(
    op: SimdOperand,
    state: &SimdState,
    mmx: &[[u8; 8]; 8],
    gpr: u32,
    memory: &[u8; 16],
) -> [u8; 16] {
    let mut value = [0; 16];
    match op {
        SimdOperand::Register(SimdRegister::Xmm(index)) => value = state.xmm[index as usize],
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            value[..8].copy_from_slice(&mmx[index as usize])
        }
        SimdOperand::Register(SimdRegister::Gpr(_)) => {
            value[..4].copy_from_slice(&gpr.to_le_bytes())
        }
        SimdOperand::Memory(_) => value = *memory,
        SimdOperand::None => {}
    }
    value
}

pub fn execute(
    op: SimdOperation,
    state: &mut SimdState,
    mmx: &mut [[u8; 8]; 8],
    memory: &mut [u8; 16],
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    validate_operation(op)?;
    execute_validated(
        op,
        state,
        mmx,
        memory,
        source_gpr,
        destination_gpr,
        incoming_flags,
    )
}

fn execute_validated(
    op: SimdOperation,
    state: &mut SimdState,
    mmx: &mut [[u8; 8]; 8],
    memory: &mut [u8; 16],
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    use SimdKind as K;
    if state.mxcsr & !0xffff != 0 {
        return Err(SimdError::InvalidState);
    }
    if matches!(op.kind, K::SaveExtended | K::RestoreExtended) {
        return Err(SimdError::InvalidDescriptor);
    }
    if matches!(op.kind, K::EmptyMmx | K::Fence | K::Prefetch) {
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    if op.kind == K::LoadMxcsr {
        let value = u32::from_le_bytes(memory[..4].try_into().unwrap());
        if value & !0xffff != 0 {
            return Err(SimdError::GeneralProtection(0));
        }
        state.mxcsr = value;
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    if op.kind == K::StoreMxcsr {
        memory[..4].copy_from_slice(&state.mxcsr.to_le_bytes());
        return Ok(SimdResult {
            gpr: destination_gpr,
            flags: incoming_flags,
        });
    }
    let a = read_operand(op.destination, state, mmx, destination_gpr, memory);
    let b = read_operand(op.source, state, mmx, source_gpr, memory);
    let size = usize::from(op.vector_bytes);
    let width = usize::from(op.lane_bytes);
    let mut result = SimdResult {
        gpr: destination_gpr,
        flags: incoming_flags,
    };
    if op.kind == K::Packed(PackedKind::Test) {
        result.flags = predicates::flags(&a, &b, BitTestKind::Bits, incoming_flags);
        return Ok(result);
    }
    let mut exception_flags = 0;
    let out = match op.kind {
        K::Move => {
            let mut out = [0; 16];
            let length = width.min(size);
            out[..length].copy_from_slice(&b[..length]);
            out
        }
        K::MoveScalar => {
            let mut out = if matches!(op.source, SimdOperand::Memory(_)) {
                [0; 16]
            } else {
                a
            };
            out[..width].copy_from_slice(&b[..width]);
            out
        }
        K::MoveLow | K::MoveHigh => {
            let mut out = a;
            let high = op.kind == K::MoveHigh;
            if op.writes_memory() {
                out[..8].copy_from_slice(&b[if high { 8 } else { 0 }..if high { 16 } else { 8 }]);
            } else {
                let start = if high { 8 } else { 0 };
                out[start..start + 8].copy_from_slice(&b[..8]);
            }
            out
        }
        K::MaskedStore => {
            result.gpr = (0..size).fold(0, |mask, index| mask | u32::from(b[index] >> 7) << index);
            a
        }
        K::Packed(kind) => packed::packed(kind, a, b, size, width, op.immediate),
        K::Crypto(kind) => crypto::execute(kind, a, b, op.immediate),
        K::Shuffle(kind) => {
            let mut immediate = op.immediate.unwrap_or(0);
            if kind == ShuffleKind::InsertSingle && op.reads_memory() {
                immediate &= 0x3f;
            }
            let (out, gpr) = packed::shuffle(kind, a, b, size, width, immediate, state.xmm[0]);
            if let Some(gpr) = gpr
                && matches!(op.destination, SimdOperand::Register(SimdRegister::Gpr(_)))
            {
                result.gpr = gpr;
            }
            out
        }
        K::Float(_) => {
            let (out, exceptions, flags) = float::floating(op, a, b, state.mxcsr, incoming_flags);
            exception_flags = exceptions;
            result.flags = flags;
            out
        }
        K::Convert { .. } => {
            let (out, exceptions, gpr) = float::convert(op, a, b, state.mxcsr);
            exception_flags = exceptions;
            if let Some(gpr) = gpr {
                result.gpr = gpr;
            }
            out
        }
        K::LoadMxcsr
        | K::StoreMxcsr
        | K::EmptyMmx
        | K::SaveExtended
        | K::RestoreExtended
        | K::Fence
        | K::Prefetch => unreachable!(),
    };
    if exception_flags != 0 {
        state.mxcsr |= exception_flags;
        if exception_flags & !(state.mxcsr >> 7) & 63 != 0 {
            return Err(SimdError::FloatingPoint(state.mxcsr));
        }
    }
    match op.destination {
        _ if op.kind == K::MaskedStore => {}
        SimdOperand::Register(SimdRegister::Xmm(index)) => state.xmm[index as usize] = out,
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            mmx[index as usize].copy_from_slice(&out[..8])
        }
        SimdOperand::Register(SimdRegister::Gpr(_)) => {
            if matches!(op.kind, K::Move) {
                result.gpr = packed::unsigned(&out, 0, 4) as u32;
            }
        }
        SimdOperand::Memory(_) => memory[..usize::from(op.memory_bytes)]
            .copy_from_slice(&out[..usize::from(op.memory_bytes)]),
        SimdOperand::None => {}
    }
    if op.kind == K::MaskedStore {
        *memory = out;
    }
    Ok(result)
}

pub fn execute_descriptor(
    arena: &mut [u8],
    descriptor: u32,
    operands: u32,
    immediate: u32,
    source_gpr: u32,
    destination_gpr: u32,
    incoming_flags: u32,
) -> Result<SimdResult, SimdError> {
    use crate::abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{
            SIMD_SIZE, X87_REGISTERS_OFFSET, X87_STATUS_OFFSET, X87_TAG_OFFSET, decode_simd,
            encode_simd,
        },
    };
    let op = decode_descriptor(descriptor, operands, immediate)?;
    if matches!(op.kind, SimdKind::SaveExtended | SimdKind::RestoreExtended) {
        return extended_descriptor(arena, op, incoming_flags, destination_gpr);
    }
    let mut state = decode_simd(
        arena
            .get(SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE)
            .ok_or(SimdError::InvalidState)?,
    )
    .map_err(|_| SimdError::InvalidState)?;
    if op.uses_mmx() || op.kind == SimdKind::EmptyMmx {
        let x87 =
            crate::abi::x86::decode_x87(&arena[X87_OFFSET..X87_OFFSET + crate::abi::x86::X87_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
        if x87.status & !x87.control & 63 != 0 {
            let status = x87.status | 0x8080;
            arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                .copy_from_slice(&status.to_le_bytes());
            return Err(SimdError::X87FloatingPoint(status));
        }
    }
    let mut mmx = std::array::from_fn(|index| {
        arena[X87_OFFSET + X87_REGISTERS_OFFSET + index * 10
            ..X87_OFFSET + X87_REGISTERS_OFFSET + index * 10 + 8]
            .try_into()
            .unwrap()
    });
    let mut memory: [u8; 16] = arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16]
        .try_into()
        .unwrap();
    let result = execute_validated(
        op,
        &mut state,
        &mut mmx,
        &mut memory,
        source_gpr,
        destination_gpr,
        incoming_flags,
    );
    match result {
        Err(SimdError::FloatingPoint(_)) => {
            encode_simd(&state, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
        }
        Ok(_) if !op.writes_memory() => {
            encode_simd(&state, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| SimdError::InvalidState)?;
            if op.uses_mmx() {
                if let SimdOperand::Register(SimdRegister::Mmx(index)) = op.destination {
                    let start = X87_OFFSET + X87_REGISTERS_OFFSET + usize::from(index) * 10;
                    arena[start..start + 8].copy_from_slice(&mmx[index as usize]);
                    arena[start + 8..start + 10].copy_from_slice(&[255; 2]);
                }
                arena[X87_OFFSET + X87_TAG_OFFSET..X87_OFFSET + X87_TAG_OFFSET + 2].fill(0);
                let sw = u16::from_le_bytes(
                    arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                        .try_into()
                        .unwrap(),
                ) & !0x3800;
                arena[X87_OFFSET + X87_STATUS_OFFSET..X87_OFFSET + X87_STATUS_OFFSET + 2]
                    .copy_from_slice(&sw.to_le_bytes());
            }
            if op.kind == SimdKind::EmptyMmx {
                arena[X87_OFFSET + X87_TAG_OFFSET..X87_OFFSET + X87_TAG_OFFSET + 2].fill(255);
            }
        }
        Ok(_) => arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].copy_from_slice(&memory),
        Err(_) => {}
    }
    result
}

pub fn save_extended(x87: &super::X87State, simd: &SimdState, output: &mut [u8; 512]) {
    output[..464].fill(0);
    super::xstate::legacy::save_x87(x87, output);
    super::xstate::legacy::save_simd(simd, output);
}

pub fn restore_extended(input: &[u8; 512]) -> Result<(super::X87State, SimdState), SimdError> {
    let mxcsr = u32::from_le_bytes(input[24..28].try_into().unwrap());
    super::xstate::validate_mxcsr(mxcsr).map_err(|_| SimdError::GeneralProtection(0))?;
    Ok((
        super::xstate::legacy::restore_x87(input),
        SimdState {
            mxcsr,
            xmm: super::xstate::legacy::restore_xmm(input),
        },
    ))
}

fn extended_descriptor(
    arena: &mut [u8],
    op: SimdOperation,
    flags: u32,
    gpr: u32,
) -> Result<SimdResult, SimdError> {
    use crate::abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_SIZE, X87_SIZE, decode_simd, decode_x87, encode_simd, encode_x87},
    };
    if arena.len() < SIMD_OFFSET + SIMD_SIZE {
        return Err(SimdError::InvalidState);
    }
    if op.kind == SimdKind::SaveExtended {
        let x87 = decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        let simd = decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        let output: &mut [u8; 512] = (&mut arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 512])
            .try_into()
            .unwrap();
        save_extended(&x87, &simd, output);
    } else {
        let (x87, simd) = restore_extended(
            (&arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 512])
                .try_into()
                .unwrap(),
        )?;
        encode_x87(&x87, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
        encode_simd(&simd, &mut arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
            .map_err(|_| SimdError::InvalidState)?;
    }
    Ok(SimdResult { gpr, flags })
}
