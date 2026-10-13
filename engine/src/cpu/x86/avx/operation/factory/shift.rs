use super::*;
pub(super) fn decode(kind: u8, s: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let semantic = match kind {
        1 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        2 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRightSigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        3 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        4 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        5 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRightSigned, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        6 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        7 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        8 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::bytes(false),
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        9 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        10 => {
            s.validate(
                D | A,
                MemoryPolicy::Forbidden,
                MemoryPolicy::Forbidden,
                true,
                immediate,
            )?;
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::bytes(true),
                    s.vector(0),
                    s.vector(1),
                    ShiftCount::Immediate(immediate),
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRight, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRightSigned, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftRightSigned, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 2)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 4)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
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
            AvxSemantic::Shift(
                ShiftInstruction::new(
                    ShiftOperation::lanes(PackedKind::ShiftLeft, 8)
                        .ok_or(AvxDecodeError::Operands)?,
                    s.vector(0),
                    s.vector(1),
                    if s.source_memory() {
                        ShiftCount::Memory
                    } else {
                        ShiftCount::Register(s.vector(2))
                    },
                )
                .ok_or(AvxDecodeError::Operands)?,
            )
        }
        _ => return Err(AvxDecodeError::Family),
    };
    Ok(semantic)
}
