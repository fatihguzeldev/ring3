use super::*;

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let width = if kind & 0x80 != 0 {
        VectorWidth::Ymm
    } else {
        VectorWidth::Xmm
    };
    let normalized = kind & 0x7f;
    let (operation, immediate_count) = match normalized {
        1 => (ShiftOperation::lanes(PackedKind::ShiftRight, 2), true),
        2 => (ShiftOperation::lanes(PackedKind::ShiftRightSigned, 2), true),
        3 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 2), true),
        4 => (ShiftOperation::lanes(PackedKind::ShiftRight, 4), true),
        5 => (ShiftOperation::lanes(PackedKind::ShiftRightSigned, 4), true),
        6 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 4), true),
        7 => (ShiftOperation::lanes(PackedKind::ShiftRight, 8), true),
        8 => (Some(ShiftOperation::bytes(false)), true),
        9 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 8), true),
        10 => (Some(ShiftOperation::bytes(true)), true),
        11 => (ShiftOperation::lanes(PackedKind::ShiftRight, 2), false),
        12 => (ShiftOperation::lanes(PackedKind::ShiftRight, 4), false),
        13 => (ShiftOperation::lanes(PackedKind::ShiftRight, 8), false),
        14 => (
            ShiftOperation::lanes(PackedKind::ShiftRightSigned, 2),
            false,
        ),
        15 => (
            ShiftOperation::lanes(PackedKind::ShiftRightSigned, 4),
            false,
        ),
        16 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 2), false),
        17 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 4), false),
        18 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 8), false),
        19 => (ShiftOperation::lanes(PackedKind::ShiftRight, 4), false),
        20 => (ShiftOperation::lanes(PackedKind::ShiftRight, 8), false),
        21 => (
            ShiftOperation::lanes(PackedKind::ShiftRightSigned, 4),
            false,
        ),
        22 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 4), false),
        23 => (ShiftOperation::lanes(PackedKind::ShiftLeft, 8), false),
        _ => return Err(AvxDecodeError::Family),
    };
    let operation = operation.ok_or(AvxDecodeError::Operands)?;
    let count = if immediate_count {
        slots.validate(
            D | A,
            MemoryPolicy::Forbidden,
            MemoryPolicy::Forbidden,
            true,
            immediate,
        )?;
        ShiftCount::Immediate(immediate)
    } else {
        slots.validate(
            D | A | B,
            MemoryPolicy::Optional,
            MemoryPolicy::Forbidden,
            false,
            immediate,
        )?;
        if normalized >= 19 {
            ShiftCount::Elements(if slots.source_memory() {
                MoveSource::Memory
            } else {
                MoveSource::Register(slots.vector(2))
            })
        } else if slots.source_memory() {
            ShiftCount::Memory
        } else {
            ShiftCount::Register(slots.vector(2))
        }
    };
    let instruction =
        ShiftInstruction::with_width(width, operation, slots.vector(0), slots.vector(1), count)
            .ok_or(AvxDecodeError::Operands)?;
    Ok(AvxSemantic::Shift(instruction))
}

#[cfg(test)]
#[path = "shift/uniform/native.rs"]
mod uniform;

#[cfg(test)]
#[path = "shift/variable/native.rs"]
mod variable;
