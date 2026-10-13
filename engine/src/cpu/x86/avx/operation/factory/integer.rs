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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::GreaterSigned, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::GreaterSigned, 2)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::GreaterSigned, 4)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Equal, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Equal, 2)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Equal, 4)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Add, 8)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::SubtractUnsignedSaturate, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::SubtractUnsignedSaturate, 2)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumUnsigned, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AddUnsignedSaturate, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AddUnsignedSaturate, 2)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumUnsigned, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AverageUnsigned, 1)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AverageUnsigned, 2)
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
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::SubtractSignedSaturate, 1)
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
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::SubtractSignedSaturate, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
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
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumSigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        19 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AddSignedSaturate, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        20 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::AddSignedSaturate, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        21 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumSigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        22 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Subtract, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        23 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Subtract, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        24 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Subtract, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        25 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Subtract, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        26 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Add, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        27 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Add, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        28 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Add, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        29 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Sign, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        30 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Sign, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        31 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Sign, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        32 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Absolute, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        33 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Absolute, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        34 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Absolute, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        35 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::Equal, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        36 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::GreaterSigned, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        37 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumSigned, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        38 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumSigned, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        39 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumUnsigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        40 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MinimumUnsigned, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        41 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumSigned, 1)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        42 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumSigned, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        43 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumUnsigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        44 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Integer(
                IntegerInstruction::new(
                    IntegerLaneOperation::new(PackedKind::MaximumUnsigned, 4)
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
