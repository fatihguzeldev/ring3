use super::*;

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let ordinal = kind
        .checked_sub(1)
        .filter(|value| *value < 96)
        .ok_or(AvxDecodeError::Family)?;
    // four ordinary operations have six shapes; alternating operations have four packed shapes.
    let (operation, order, shape) = if ordinal < 72 {
        (ordinal / 18, ordinal % 18 / 6, ordinal % 6)
    } else {
        (
            4 + (ordinal - 72) / 12,
            (ordinal - 72) % 12 / 4,
            (ordinal - 72) % 4,
        )
    };
    let kind = match operation {
        0 => FmaKind::Add,
        1 => FmaKind::Subtract,
        2 => FmaKind::NegativeAdd,
        3 => FmaKind::NegativeSubtract,
        4 => FmaKind::AddSubtract,
        5 => FmaKind::SubtractAdd,
        _ => unreachable!(),
    };
    let order = match order {
        0 => FmaOrder::Order132,
        1 => FmaOrder::Order213,
        2 => FmaOrder::Order231,
        _ => unreachable!(),
    };
    let shape = match shape {
        0 => FmaShape::Packed {
            width: VectorWidth::Xmm,
            element: Element::Single,
        },
        1 => FmaShape::Packed {
            width: VectorWidth::Ymm,
            element: Element::Single,
        },
        2 => FmaShape::Packed {
            width: VectorWidth::Xmm,
            element: Element::Double,
        },
        3 => FmaShape::Packed {
            width: VectorWidth::Ymm,
            element: Element::Double,
        },
        4 => FmaShape::Scalar {
            element: Element::Single,
        },
        5 => FmaShape::Scalar {
            element: Element::Double,
        },
        _ => unreachable!(),
    };
    slots.validate(
        D | A | B,
        MemoryPolicy::Optional,
        MemoryPolicy::Forbidden,
        false,
        immediate,
    )?;
    Ok(AvxSemantic::Fma(FmaInstruction {
        kind,
        order,
        shape,
        destination: slots.vector(0),
        second: slots.vector(1),
        third: slots.source(),
    }))
}

#[cfg(test)]
mod native;
