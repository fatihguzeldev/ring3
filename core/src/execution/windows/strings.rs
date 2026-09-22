use super::super::Access;
use super::{DispatchError, GuestMemory, MemoryError, guest, thread};

#[derive(Clone, Copy)]
pub(super) enum Call {
    Copy,
    CopyTerminated,
    Append,
    Length,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0xe4 => Some(Self::Copy),
            0xf0 => Some(Self::CopyTerminated),
            0xf4 => Some(Self::Append),
            0x28c => Some(Self::Length),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Copy => 3,
            Self::Length => 1,
            Self::CopyTerminated | Self::Append => 2,
        }
    }

    pub(super) fn dispatch(
        self,
        memory: &mut GuestMemory,
        arguments: &[u32],
    ) -> Result<u32, DispatchError> {
        match self {
            Self::Copy | Self::CopyTerminated => copy(
                memory,
                arguments[0],
                arguments[1],
                arguments.get(2).copied().unwrap_or(u32::MAX),
            ),
            Self::Append => append(memory, arguments[0], arguments[1]),
            Self::Length if arguments[0] == 0 => Ok(0),
            Self::Length => append_address(memory, arguments[0]).map(|end| end - arguments[0]),
        }
    }
}

fn copy(
    memory: &mut GuestMemory,
    destination: u32,
    source: u32,
    count: u32,
) -> Result<u32, DispatchError> {
    let prepared = prepare(memory, destination, source, count).map(|bytes| (destination, bytes));
    complete(memory, destination, prepared)
}

fn append(memory: &mut GuestMemory, destination: u32, source: u32) -> Result<u32, DispatchError> {
    let prepared = append_address(memory, destination)
        .and_then(|output| prepare(memory, output, source, u32::MAX).map(|bytes| (output, bytes)));
    complete(memory, destination, prepared)
}

fn append_address(memory: &GuestMemory, destination: u32) -> Result<u32, DispatchError> {
    for offset in 0..65536 {
        let address = destination
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(address);
        }
    }
    Err(DispatchError::Unsupported)
}

fn complete(
    memory: &mut GuestMemory,
    destination: u32,
    prepared: Result<(u32, Vec<u8>), DispatchError>,
) -> Result<u32, DispatchError> {
    match prepared {
        Ok((output, bytes)) => {
            memory.write(u64::from(output), &bytes)?;
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
