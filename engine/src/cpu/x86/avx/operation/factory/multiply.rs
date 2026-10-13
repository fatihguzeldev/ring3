use super::*;

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let width = if kind & 0x80 == 0 {
        VectorWidth::Xmm
    } else {
        VectorWidth::Ymm
    };
    let kind = kind & 0x7f;
    if width == VectorWidth::Ymm && !matches!(kind, 1..=16) {
        return Err(AvxDecodeError::Family);
    }
    let (operation, bytes) = match kind {
        1 => (PackedKind::MultiplyLow, 2),
        2 => (PackedKind::MultiplyHighUnsigned, 2),
        3 => (PackedKind::MultiplyHighSigned, 2),
        4 => (PackedKind::MultiplyUnsignedDword, 4),
        5 => (PackedKind::MultiplyAddWords, 2),
        6 => (PackedKind::SumAbsoluteByteDifferences, 1),
        7 => (PackedKind::HorizontalAdd, 2),
        8 => (PackedKind::HorizontalAdd, 4),
        9 => (PackedKind::HorizontalAddSignedSaturate, 2),
        10 => (PackedKind::MultiplyAddUnsignedSignedBytes, 1),
        11 => (PackedKind::HorizontalSubtract, 2),
        12 => (PackedKind::HorizontalSubtract, 4),
        13 => (PackedKind::HorizontalSubtractSignedSaturate, 2),
        14 => (PackedKind::MultiplyHighRoundSigned, 2),
        15 => (PackedKind::MultiplySignedDword, 4),
        16 => (PackedKind::MultiplyLow, 4),
        17 => (PackedKind::MinimumPositionUnsignedWord, 2),
        18 => (PackedKind::MultipleSumAbsoluteByteDifferences, 1),
        _ => return Err(AvxDecodeError::Family),
    };
    let unary = kind == 17;
    slots.validate(
        if unary { D | B } else { D | A | B },
        MemoryPolicy::Optional,
        MemoryPolicy::Forbidden,
        kind == 18,
        immediate,
    )?;
    let operation =
        MultiplyOperation::new(operation, bytes, if kind == 18 { immediate } else { 0 })
            .ok_or(AvxDecodeError::Operands)?;
    let instruction = MultiplyInstruction::with_width(
        width,
        operation,
        slots.vector(0),
        (!unary).then_some(slots.vector(1)),
        slots.source(),
    )
    .ok_or(AvxDecodeError::Operands)?;
    Ok(AvxSemantic::Multiply(instruction))
}

#[cfg(test)]
#[path = "multiply/width/native.rs"]
mod width;
