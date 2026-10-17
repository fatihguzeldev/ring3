use crate::{
    abi::{AbiError, header},
    windows::imm32::{InputComposition32, InputCompositionEvent32 as Event},
};

pub fn decode_composition_event(input: &[u8]) -> Result<Event, AbiError> {
    if input.len() < 32 {
        return Err(AbiError::Length);
    }
    let (_, kind) = header::validate_versions_with_metadata(input, *b"R3IM", input.len(), &[1])?;
    let units = header::read_u32(input, 16) as usize;
    let attributes = header::read_u32(input, 20) as usize;
    let clauses = header::read_u32(input, 24) as usize;
    let cursor = header::read_u32(input, 28);
    let text_end = units
        .checked_mul(2)
        .and_then(|n| n.checked_add(32))
        .ok_or(AbiError::Length)?;
    let attributes_end = text_end.checked_add(attributes).ok_or(AbiError::Length)?;
    let expected = clauses
        .checked_mul(4)
        .and_then(|n| attributes_end.checked_add(n))
        .ok_or(AbiError::Length)?;
    if input.len() != expected {
        return Err(AbiError::Length);
    }
    match kind {
        1 | 4 if units == 0 && attributes == 0 && clauses == 0 && cursor == 0 => Ok(if kind == 1 {
            Event::Begin
        } else {
            Event::Cancel
        }),
        2 => {
            let value = InputComposition32 {
                text: wide(&input[32..text_end]),
                attributes: input[text_end..attributes_end].to_vec(),
                clauses: input[attributes_end..]
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                    .collect(),
                cursor,
                ..Default::default()
            };
            value
                .check(crate::windows::imm32::MAX_COMPOSITION_UNITS)
                .map_err(|_| AbiError::Flags)?;
            Ok(Event::Update(value))
        }
        3 if attributes == 0
            && clauses == 0
            && cursor == 0
            && units <= crate::windows::imm32::MAX_COMPOSITION_UNITS =>
        {
            Ok(Event::Commit(wide(&input[32..text_end])))
        }
        _ => Err(AbiError::Flags),
    }
}
fn wide(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[cfg(test)]
mod tests;
