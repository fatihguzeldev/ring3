use super::{DispatchError, GuestMemory, MemoryError};

pub(super) fn reverse_search(
    memory: &GuestMemory,
    pointer: u32,
    character: u32,
) -> Result<u32, DispatchError> {
    if pointer == 0 {
        return Err(DispatchError::Unsupported);
    }
    let mut found = 0;
    let character = character.to_le_bytes()[0];
    for offset in 0..65536 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == character {
            found = address;
        }
        if byte[0] == 0 {
            return Ok(found);
        }
    }
    Err(DispatchError::Unsupported)
}
