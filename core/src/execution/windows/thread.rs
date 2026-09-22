use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

pub(super) const CURRENT_ID: u32 = 1;
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
        (0x24, CURRENT_ID),
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

pub(super) fn current_id(memory: &GuestMemory) -> Result<u32, MemoryError> {
    let mut value = [0];
    guest::read_words(memory, BASE + 0x24, &mut value)?;
    Ok(value[0])
}

pub(super) fn check_last_error_write(memory: &GuestMemory) -> Result<(), MemoryError> {
    guest::check(memory, LAST_ERROR, 4, super::super::Access::Write)
}

#[derive(Clone, Copy)]
pub(super) enum PriorityCall {
    Set,
    Get,
}

impl PriorityCall {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x254 => Some(Self::Set),
            0x258 => Some(Self::Get),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Set => 2,
            Self::Get => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Priority(u32);

impl Priority {
    pub(super) fn dispatch(
        &mut self,
        call: PriorityCall,
        arguments: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, super::DispatchError> {
        if arguments[0] != u32::MAX - 1 {
            set_last_error(memory, 6)?;
            return Ok(if matches!(call, PriorityCall::Get) {
                0x7fff_ffff
            } else {
                0
            });
        }
        if matches!(call, PriorityCall::Get) {
            return Ok(self.0);
        }
        let value = arguments[1];
        if !matches!(i32::from_ne_bytes(value.to_ne_bytes()), -15 | -2..=2 | 15) {
            return Err(super::DispatchError::Unsupported);
        }
        self.0 = value;
        Ok(1)
    }
}
