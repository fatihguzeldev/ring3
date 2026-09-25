use super::{DispatchError, ENVIRON, GuestMemory, MemoryError, guest};

pub(super) fn get(memory: &GuestMemory, source: u32) -> Result<u32, DispatchError> {
    if source == 0 {
        return Err(DispatchError::Unsupported);
    }
    let name = super::super::environment::read_name(memory, source)?;
    if name.is_empty() {
        return Ok(0);
    }
    let mut table = [0];
    guest::read_words(memory, ENVIRON, &mut table)?;
    if table[0] == 0 {
        return Ok(0);
    }
    for index in 0..=256 {
        let slot = table[0]
            .checked_add(index * 4)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut entry = [0];
        guest::read_words(memory, slot, &mut entry)?;
        if entry[0] == 0 {
            return Ok(0);
        }
        if index == 256 {
            break;
        }
        if matches_name(memory, entry[0], &name)? {
            let offset = u32::try_from(name.len() + 1).expect("bounded environment name");
            return Ok(entry[0]
                .checked_add(offset)
                .ok_or(MemoryError::AddressOverflow)?);
        }
    }
    Err(DispatchError::Unsupported)
}

fn matches_name(memory: &GuestMemory, entry: u32, name: &[u8]) -> Result<bool, DispatchError> {
    for (offset, expected) in name
        .iter()
        .copied()
        .chain(std::iter::once(b'='))
        .enumerate()
    {
        let offset = u32::try_from(offset).expect("bounded environment name");
        let address = entry
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if !byte[0].eq_ignore_ascii_case(&expected) {
            return Ok(false);
        }
    }
    Ok(true)
}
