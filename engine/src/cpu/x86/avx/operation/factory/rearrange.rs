use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let width = if kind & 0x80 != 0 {
        if !matches!(kind & 0x7f, 16..=21 | 23..=28) {
            return Err(AvxDecodeError::Family);
        }
        VectorWidth::Ymm
    } else {
        VectorWidth::Xmm
    };
    let kind = kind & 0x7f;
    let semantic = match kind {
        1 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackLow, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackLow, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackLow, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::PackSigned, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::PackUnsigned, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackHigh, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackHigh, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackHigh, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::PackSigned, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackLow, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::UnpackHigh, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        12 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::Dwords, 4, immediate)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        13 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::HighWords, 2, immediate)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        14 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::LowWords, 2, immediate)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::Bytes, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        16 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendBytes, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendBytes, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        18 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendBytes, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        19 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendWords, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        20 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendWords, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        21 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::SignExtendDwords, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
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
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::PackUnsigned, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        23 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendBytes, 2, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        24 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendBytes, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        25 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendBytes, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        26 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendWords, 4, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        27 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendWords, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        28 => {
            s.validate(
                D | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::ZeroExtendDwords, 8, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    None,
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        29 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::BlendImmediate, 2, immediate)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        30 => {
            s.validate(
                D | A | B,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::AlignRight, 1, immediate)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    None,
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        31 => {
            s.validate(
                D | A | B | M,
                MemoryPolicy::Optional,
                MemoryPolicy::Forbidden,
                false,
                immediate,
            )?;
            AvxSemantic::Rearrange(
                RearrangeInstruction::new(
                    RearrangeOperation::new(ShuffleKind::BlendVariable, 1, 0)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    Some(s.vector(1)),
                    s.source(),
                    Some(s.vector(3)),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        _ => return Err(AvxDecodeError::Family),
    };
    match semantic {
        AvxSemantic::Rearrange(value) => Ok(AvxSemantic::Rearrange(
            value.with_width(width).ok_or(AvxDecodeError::Family)?,
        )),
        _ => unreachable!("rearrange factory constructs only rearrange instructions"),
    }
}

#[cfg(test)]
#[path = "rearrange/widen/native.rs"]
mod widen_tests;
