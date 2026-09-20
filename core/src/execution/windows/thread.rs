use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

pub(super) const BASE: u32 = 0x7ffd_e000;
const LAST_ERROR: u32 = BASE + 0x34;

pub(super) fn initialize(
    memory: &mut GuestMemory,
    stack_limit: u32,
    stack_base: u32,
) -> Result<(), MemoryError> {
    memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
    for (offset, value) in [
        (0, u32::MAX),
        (4, stack_base),
        (8, stack_limit),
        (0x18, BASE),
    ] {
        guest::write_word(memory, BASE + offset, value)?;
    }
    Ok(())
}

pub(super) fn last_error(memory: &GuestMemory) -> Result<u32, MemoryError> {
    let mut value = [0];
    guest::read_words(memory, LAST_ERROR, &mut value)?;
    Ok(value[0])
}

pub(super) fn set_last_error(memory: &mut GuestMemory, value: u32) -> Result<(), MemoryError> {
    guest::write_word(memory, LAST_ERROR, value)
}
