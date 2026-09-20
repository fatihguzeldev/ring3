use super::super::Access;
use super::{GuestMemory, MemoryError};

pub(super) fn check(
    memory: &GuestMemory,
    address: u32,
    length: usize,
    access: Access,
) -> Result<(), MemoryError> {
    if u64::from(address) + length as u64 > 1_u64 << 32 {
        return Err(MemoryError::AddressOverflow);
    }
    memory.check_access(u64::from(address), length, access)
}

pub(super) fn read_words(
    memory: &GuestMemory,
    address: u32,
    output: &mut [u32],
) -> Result<(), MemoryError> {
    check(memory, address, output.len() * 4, Access::Read)?;
    for (index, value) in (0_u64..).zip(output) {
        let mut bytes = [0; 4];
        memory.read(u64::from(address) + index * 4, &mut bytes)?;
        *value = u32::from_le_bytes(bytes);
    }
    Ok(())
}

pub(super) fn write_word(
    memory: &mut GuestMemory,
    address: u32,
    value: u32,
) -> Result<(), MemoryError> {
    check(memory, address, 4, Access::Write)?;
    memory.write(u64::from(address), &value.to_le_bytes())
}
