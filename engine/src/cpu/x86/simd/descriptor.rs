use super::{
    ConvertKind, CryptoKind, FloatKind, PackedKind, ShuffleKind, SimdError, SimdKind, SimdOperand,
    SimdOperation, SimdRegister,
};
use crate::cpu::x86::{Register32, ir::EffectiveAddress};

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
    pub fn descriptor(self) -> Result<(u32, u32, u32), SimdError> {
        validate_operation(self)?;
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
            .ok_or(SimdError::InvalidDescriptor)? as u32;
        let memory = [0, 1, 2, 4, 8, 16, 512]
            .iter()
            .position(|width| *width == self.memory_bytes)
            .ok_or(SimdError::InvalidDescriptor)? as u32;
        let descriptor = (kind & 15)
            | ((kind & 16) << 20)
            | (sub << 4)
            | (lane << 10)
            | (u32::from(self.vector_bytes == 16) << 13)
            | (u32::from(self.vector_bytes == 4) << 14)
            | (memory << 15)
            | (u32::from(self.aligned) << 20)
            | (u32::from(self.scalar) << 21)
            | (u32::from(self.immediate.is_some()) << 22)
            | (u32::from(truncate) << 23);
        Ok((
            descriptor,
            operand_code(self.destination) | (operand_code(self.source) << 8),
            u32::from(self.immediate.unwrap_or(0)),
        ))
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

pub(super) fn validate_operation(op: SimdOperation) -> Result<(), SimdError> {
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
