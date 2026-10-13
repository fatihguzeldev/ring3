use super::*;

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let width = match kind {
        1 | 3 => VectorWidth::Xmm,
        2 | 4 => VectorWidth::Ymm,
        _ => return Err(AvxDecodeError::Family),
    };
    let instruction = if kind <= 2 {
        slots.validate(
            D | B,
            MemoryPolicy::Optional,
            MemoryPolicy::Forbidden,
            false,
            immediate,
        )?;
        F16cInstruction::Widen {
            width,
            destination: slots.vector(0),
            source: slots.source(),
        }
    } else {
        slots.validate(
            D | B,
            MemoryPolicy::Forbidden,
            MemoryPolicy::Optional,
            true,
            immediate,
        )?;
        F16cInstruction::Narrow {
            width,
            source: slots.vector(2),
            destination: if slots.target_memory() {
                F16cTarget::Memory
            } else {
                F16cTarget::Register(slots.vector(0))
            },
            immediate,
        }
    };
    Ok(AvxSemantic::F16c(instruction))
}

#[cfg(test)]
mod tests;
