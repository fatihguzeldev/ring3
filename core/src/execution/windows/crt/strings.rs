use super::{Access, DispatchError, ERRNO, GuestMemory, MemoryError, buffers, guest, heap};

pub(super) fn find(
    memory: &GuestMemory,
    source: u32,
    character: u32,
) -> Result<u32, DispatchError> {
    let character = character.to_le_bytes()[0];
    for offset in 0..65536 {
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == character {
            return Ok(address);
        }
        if byte[0] == 0 {
            return Ok(0);
        }
    }
    Err(DispatchError::Unsupported)
}

pub(super) fn copy(
    memory: &mut GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    if count == 0 {
        return Ok(destination);
    }
    guest::check(
        memory,
        destination,
        usize::try_from(count).expect("u32 count fits target usize"),
        Access::Write,
    )?;
    let copied = prefix_length(memory, source, count)?;
    let read_length = copied + u32::from(copied < count);
    if u64::from(source) < u64::from(destination) + u64::from(count)
        && u64::from(destination) < u64::from(source) + u64::from(read_length)
    {
        return Err(DispatchError::Unsupported);
    }
    buffers::copy(memory, destination, source, copied)?;
    if copied < count {
        memory.fill(
            u64::from(destination) + u64::from(copied),
            usize::try_from(count - copied).expect("u32 count fits target usize"),
            0,
        )?;
    }
    Ok(destination)
}

fn prefix_length(memory: &GuestMemory, source: u32, count: u32) -> Result<u32, DispatchError> {
    for offset in 0..count {
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(offset);
        }
    }
    Ok(count)
}

pub(super) fn length(memory: &GuestMemory, source: u32) -> Result<u32, DispatchError> {
    for offset in 0..65536 {
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(offset);
        }
    }
    Err(DispatchError::Unsupported)
}

pub(super) fn duplicate(
    memory: &mut GuestMemory,
    heap: &mut heap::Heap,
    source: u32,
) -> Result<u32, DispatchError> {
    if source == 0 {
        return Ok(0);
    }
    let bytes = terminated_bytes(memory, source)?;
    let length = u32::try_from(bytes.len()).expect("bounded string length fits u32");
    let Some(pointer) = heap.allocate_crt(length, memory)? else {
        guest::write_word(memory, ERRNO, 12)?;
        return Ok(0);
    };
    memory
        .write(u64::from(pointer), &bytes)
        .expect("new crt allocation is writable");
    Ok(pointer)
}

fn terminated_bytes(memory: &GuestMemory, source: u32) -> Result<Vec<u8>, DispatchError> {
    let mut bytes = Vec::new();
    for offset in 0..65536 {
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        bytes.push(byte[0]);
        if byte[0] == 0 {
            return Ok(bytes);
        }
    }
    Err(DispatchError::Unsupported)
}
