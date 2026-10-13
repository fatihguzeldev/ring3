use super::*;
use crate::cpu::x86::avx::{GatherGeometry, GatherInstruction, GatherScale};

pub(super) fn decode(kind: u8, slots: Slots, immediate: u8) -> Result<AvxSemantic, AvxDecodeError> {
    let geometry = GatherGeometry::from_id(kind).ok_or(AvxDecodeError::Family)?;
    slots.validate(
        D | A | M,
        MemoryPolicy::Required,
        MemoryPolicy::Forbidden,
        true,
        immediate,
    )?;
    let destination = slots.vector(0);
    let index = slots.vector(1);
    let mask = slots.vector(3);
    if destination == index || destination == mask || index == mask {
        return Err(AvxDecodeError::Operands);
    }
    let scale = GatherScale::from_power(immediate).ok_or(AvxDecodeError::Immediate)?;
    let value = GatherInstruction::new(geometry, destination, index, mask, scale)
        .ok_or(AvxDecodeError::Operands)?;
    Ok(AvxSemantic::Gather(value))
}

#[cfg(test)]
#[path = "gather/native.rs"]
mod tests;
