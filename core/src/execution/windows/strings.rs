use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, guest, thread};

pub(super) fn copy(
    memory: &mut GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    match prepare(memory, destination, source, count) {
        Ok(bytes) => {
            memory.write(u64::from(destination), &bytes)?;
            Ok(destination)
        }
        Err(DispatchError::Memory(_)) => {
            thread::set_last_error(memory, 87)?;
            Ok(0)
        }
        Err(error) => Err(error),
    }
}

fn prepare(
    memory: &GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<Vec<u8>, DispatchError> {
    let mut bytes = Vec::new();
    if count == 0 {
        return Ok(bytes);
    }
    let mut read_length = 0;
    for offset in 0..count - 1 {
        if bytes.len() == 65536 {
            return Err(DispatchError::Unsupported);
        }
        let address = source
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        read_length = u64::from(offset) + 1;
        if byte[0] == 0 {
            break;
        }
        bytes.push(byte[0]);
    }
    if bytes.len() == 65536 {
        return Err(DispatchError::Unsupported);
    }
    bytes.push(0);
    if read_length != 0
        && u64::from(source) < u64::from(destination) + bytes.len() as u64
        && u64::from(destination) < u64::from(source) + read_length
    {
        return Err(DispatchError::Unsupported);
    }
    guest::check(memory, destination, bytes.len(), Access::Write)?;
    Ok(bytes)
}
