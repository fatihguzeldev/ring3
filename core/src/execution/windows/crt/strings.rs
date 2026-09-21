use super::{DispatchError, ERRNO, GuestMemory, MemoryError, guest, heap};

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
