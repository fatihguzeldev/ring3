use super::*;

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let (kind, lane_bytes) = match kind {
        1 => (PackedKind::GreaterSigned, 1),
        2 => (PackedKind::GreaterSigned, 2),
        3 => (PackedKind::GreaterSigned, 4),
        4 => (PackedKind::Equal, 1),
        5 => (PackedKind::Equal, 2),
        6 => (PackedKind::Equal, 4),
        7 => (PackedKind::Add, 8),
        8 => (PackedKind::SubtractUnsignedSaturate, 1),
        9 => (PackedKind::SubtractUnsignedSaturate, 2),
        10 => (PackedKind::MinimumUnsigned, 1),
        11 => (PackedKind::AddUnsignedSaturate, 1),
        12 => (PackedKind::AddUnsignedSaturate, 2),
        13 => (PackedKind::MaximumUnsigned, 1),
        14 => (PackedKind::AverageUnsigned, 1),
        15 => (PackedKind::AverageUnsigned, 2),
        16 => (PackedKind::SubtractSignedSaturate, 1),
        17 => (PackedKind::SubtractSignedSaturate, 2),
        18 => (PackedKind::MinimumSigned, 2),
        19 => (PackedKind::AddSignedSaturate, 1),
        20 => (PackedKind::AddSignedSaturate, 2),
        21 => (PackedKind::MaximumSigned, 2),
        22 => (PackedKind::Subtract, 1),
        23 => (PackedKind::Subtract, 2),
        24 => (PackedKind::Subtract, 4),
        25 => (PackedKind::Subtract, 8),
        26 => (PackedKind::Add, 1),
        27 => (PackedKind::Add, 2),
        28 => (PackedKind::Add, 4),
        29 => (PackedKind::Sign, 1),
        30 => (PackedKind::Sign, 2),
        31 => (PackedKind::Sign, 4),
        32 => (PackedKind::Absolute, 1),
        33 => (PackedKind::Absolute, 2),
        34 => (PackedKind::Absolute, 4),
        35 => (PackedKind::Equal, 8),
        36 => (PackedKind::GreaterSigned, 8),
        37 => (PackedKind::MinimumSigned, 1),
        38 => (PackedKind::MinimumSigned, 4),
        39 => (PackedKind::MinimumUnsigned, 2),
        40 => (PackedKind::MinimumUnsigned, 4),
        41 => (PackedKind::MaximumSigned, 1),
        42 => (PackedKind::MaximumSigned, 4),
        43 => (PackedKind::MaximumUnsigned, 2),
        44 => (PackedKind::MaximumUnsigned, 4),
        _ => return Err(AvxDecodeError::Family),
    };
    let operation = IntegerLaneOperation::new(kind, lane_bytes).ok_or(AvxDecodeError::Operands)?;
    let unary = operation.unary();
    slots.validate(
        if unary { D | B } else { D | A | B },
        MemoryPolicy::Optional,
        MemoryPolicy::Forbidden,
        false,
        immediate,
    )?;
    let first = if unary { None } else { Some(slots.vector(1)) };
    let instruction = IntegerInstruction::new(operation, slots.vector(0), first, slots.source())
        .ok_or(AvxDecodeError::Operands)?;
    Ok(AvxSemantic::Integer(instruction))
}

#[cfg(test)]
#[path = "integer/native.rs"]
mod native;
