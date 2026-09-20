use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, thread};

const START: u64 = 0x2000_0000;
const END: u64 = 0x3000_0000;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Alloc,
    Free,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x28 => Some(Self::Alloc),
            0x2c => Some(Self::Free),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Alloc => 2,
            Self::Free => 1,
        }
    }
}

#[derive(Default)]
pub(super) struct Heap {
    allocations: BTreeMap<u32, u64>,
}

impl Heap {
    pub(super) fn dispatch(
        &mut self,
        call: Call,
        arguments: &[u32],
        stack: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Alloc => self.allocate(arguments[0], arguments[1], memory),
            Call::Free => self.free(arguments[0], stack, memory),
        }
    }

    fn allocate(
        &mut self,
        flags: u32,
        size: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if flags & !0x40 != 0 {
            return Err(DispatchError::Unsupported);
        }
        let length = u64::from(size).max(1).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        if let Some(address) = memory.first_free_span(START, END, length)? {
            match memory.map_zeroed(address, length, Permissions::READ_WRITE) {
                Ok(()) => {
                    let pointer = u32::try_from(address).expect("heap window fits u32");
                    self.allocations.insert(pointer, length);
                    return Ok(pointer);
                }
                Err(MemoryError::PageLimitExceeded) => {}
                Err(error) => return Err(error.into()),
            }
        }
        thread::set_last_error(memory, 8)?;
        Ok(0)
    }

    fn free(
        &mut self,
        pointer: u32,
        stack: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if pointer == 0 {
            return Ok(0);
        }
        let Some(&length) = self.allocations.get(&pointer) else {
            thread::set_last_error(memory, 6)?;
            return Ok(pointer);
        };
        let address = u64::from(pointer);
        // dispatch must still read the return slot after the call completes.
        if address < u64::from(stack) + 8 && u64::from(stack) < address + length {
            return Err(DispatchError::Unsupported);
        }
        memory.unmap(address, length)?;
        self.allocations.remove(&pointer);
        Ok(0)
    }
}
