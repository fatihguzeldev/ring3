use super::{GuestMemory, MemoryError, PAGE_SIZE, Permissions, guest};

pub(super) const CURRENT_ID: u32 = 1;
pub(super) const BASE: u32 = 0x7ffd_e000;

#[derive(Clone, Copy)]
pub(super) struct Teb(pub(super) u32);

impl Teb {
    fn field(self, offset: u32) -> Result<u32, MemoryError> {
        self.0
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)
    }

    fn read(self, memory: &GuestMemory, offset: u32) -> Result<u32, MemoryError> {
        let mut value = [0];
        guest::read_words(memory, self.field(offset)?, &mut value)?;
        Ok(value[0])
    }

    pub(super) fn last_error(self, memory: &GuestMemory) -> Result<u32, MemoryError> {
        self.read(memory, 0x34)
    }

    pub(super) fn set_last_error(
        self,
        memory: &mut GuestMemory,
        value: u32,
    ) -> Result<(), MemoryError> {
        guest::write_word(memory, self.field(0x34)?, value)
    }

    pub(super) fn current_id(self, memory: &GuestMemory) -> Result<u32, MemoryError> {
        self.read(memory, 0x24)
    }

    pub(super) fn check_last_error_write(self, memory: &GuestMemory) -> Result<(), MemoryError> {
        guest::check(memory, self.field(0x34)?, 4, super::super::Access::Write)
    }
}

pub(super) fn initialize(
    memory: &mut GuestMemory,
    stack_limit: u32,
    stack_base: u32,
) -> Result<(), MemoryError> {
    memory.map_zeroed(u64::from(BASE), PAGE_SIZE, Permissions::READ_WRITE)?;
    initialize_contents(memory, BASE, CURRENT_ID, stack_limit, stack_base)
}

pub(super) fn initialize_contents(
    memory: &mut GuestMemory,
    base: u32,
    id: u32,
    stack_limit: u32,
    stack_base: u32,
) -> Result<(), MemoryError> {
    guest::check(memory, base, 0x38, super::super::Access::Write)?;
    for (offset, value) in [
        (0, u32::MAX),
        (4, stack_base),
        (8, stack_limit),
        (0x18, base),
        (0x24, id),
        (0x34, 0),
    ] {
        guest::write_word(memory, base + offset, value)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn set_last_error(memory: &mut GuestMemory, value: u32) -> Result<(), MemoryError> {
    Teb(BASE).set_last_error(memory, value)
}

#[cfg(test)]
pub(super) fn last_error(memory: &GuestMemory) -> Result<u32, MemoryError> {
    Teb(BASE).last_error(memory)
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
    pub(super) fn relative(&self) -> i32 {
        i32::from_ne_bytes(self.0.to_ne_bytes())
    }

    pub(super) fn dispatch(
        &mut self,
        call: PriorityCall,
        arguments: &[u32],
    ) -> Result<u32, super::DispatchError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_tebs_keep_identity_stack_bounds_and_error_state_separate() {
        let mut memory = GuestMemory::new(2);
        initialize(&mut memory, 0x1000_0000, 0x1001_0000).unwrap();
        set_last_error(&mut memory, 77).unwrap();
        memory
            .map_zeroed(0x1101_0000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        initialize_contents(&mut memory, 0x1101_0000, 2, 0x1100_0000, 0x1101_0000).unwrap();
        for (base, id, low, high, error) in [
            (BASE, 1, 0x1000_0000, 0x1001_0000, 77),
            (0x1101_0000, 2, 0x1100_0000, 0x1101_0000, 0),
        ] {
            for (offset, expected) in [
                (0, u32::MAX),
                (4, high),
                (8, low),
                (0x18, base),
                (0x24, id),
                (0x34, error),
            ] {
                let mut word = [0];
                guest::read_words(&memory, base + offset, &mut word).unwrap();
                assert_eq!(word[0], expected);
            }
        }
    }
}
