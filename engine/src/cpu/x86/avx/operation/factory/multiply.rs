use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyLow, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        2 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyHighUnsigned, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        3 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyHighSigned, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        4 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyUnsignedDword, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        5 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyAddWords, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        6 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::SumAbsoluteByteDifferences, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        7 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalAdd, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        8 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalAdd, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        9 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalAddSignedSaturate, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        10 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyAddUnsignedSignedBytes, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        11 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalSubtract, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        12 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalSubtract, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        13 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::HorizontalSubtractSignedSaturate, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        14 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyHighRoundSigned, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        15 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplySignedDword, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        16 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MultiplyLow, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        17 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(PackedKind::MinimumPositionUnsignedWord, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        18 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Multiply(
                MultiplyInstruction::new(
                    MultiplyOperation::new(
                        PackedKind::MultipleSumAbsoluteByteDifferences,
                        1,
                        immediate,
                    )
                    .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
