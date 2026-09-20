use std::collections::BTreeMap;

use super::{DispatchError, GuestMemory, MemoryError, PAGE_SIZE, Permissions, thread};

const START: u64 = 0x2000_0000;
const END: u64 = 0x3000_0000;

#[derive(Clone, Copy)]
pub(super) enum Call {
    Alloc,
    Free,
    GlobalAlloc,
    GlobalLock,
    GlobalUnlock,
    GlobalFree,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x28 => Some(Self::Alloc),
            0x2c => Some(Self::Free),
            0x78 => Some(Self::GlobalAlloc),
            0x7c => Some(Self::GlobalLock),
            0x80 => Some(Self::GlobalUnlock),
            0x84 => Some(Self::GlobalFree),
            _ => None,
        }
    }

    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Alloc | Self::GlobalAlloc => 2,
            _ => 1,
        }
    }
}

enum Kind {
    Local,
    Crt,
    GlobalFixed,
    GlobalMovable { discarded: bool, locks: u32 },
}

struct Allocation {
    base: u32,
    length: u64,
    kind: Kind,
}

impl Allocation {
    fn global(&self) -> bool {
        matches!(self.kind, Kind::GlobalFixed | Kind::GlobalMovable { .. })
    }
}

#[derive(Default)]
pub(super) struct Heap {
    allocations: BTreeMap<u32, Allocation>,
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
            Call::Alloc | Call::GlobalAlloc => self.allocate(
                matches!(call, Call::GlobalAlloc),
                arguments[0],
                arguments[1],
                memory,
            ),
            Call::Free | Call::GlobalFree => self.free(
                matches!(call, Call::GlobalFree),
                arguments[0],
                stack,
                memory,
            ),
            Call::GlobalLock => self.lock(arguments[0], memory),
            Call::GlobalUnlock => self.unlock(arguments[0], memory),
        }
    }

    fn allocate(
        &mut self,
        global: bool,
        flags: u32,
        size: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let allowed = if global { 0x7172 } else { 0x40 };
        if flags & !allowed != 0 {
            return Err(DispatchError::Unsupported);
        }
        let movable = global && flags & 2 != 0;
        let kind = if movable {
            Kind::GlobalMovable {
                discarded: size == 0,
                locks: 0,
            }
        } else if global {
            Kind::GlobalFixed
        } else {
            Kind::Local
        };
        if let Some(handle) = self.reserve(size, kind, memory)? {
            return Ok(handle);
        }
        thread::set_last_error(memory, 8)?;
        Ok(0)
    }

    fn reserve(
        &mut self,
        size: u32,
        kind: Kind,
        memory: &mut GuestMemory,
    ) -> Result<Option<u32>, MemoryError> {
        let permissions = if matches!(
            kind,
            Kind::GlobalMovable {
                discarded: true,
                ..
            }
        ) {
            Permissions::NONE
        } else {
            Permissions::READ_WRITE
        };
        let length = u64::from(size).max(1).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        if let Some(address) = memory.first_free_span(START, END, length)? {
            match memory.map_zeroed(address, length, permissions) {
                Ok(()) => {
                    let base = u32::try_from(address).expect("heap window fits u32");
                    // the tag distinguishes opaque movable handles from fixed pointers.
                    let handle = base
                        | if matches!(kind, Kind::GlobalMovable { .. }) {
                            2
                        } else {
                            0
                        };
                    self.allocations
                        .insert(handle, Allocation { base, length, kind });
                    return Ok(Some(handle));
                }
                Err(MemoryError::PageLimitExceeded) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    pub(super) fn allocate_crt(
        &mut self,
        size: u32,
        memory: &mut GuestMemory,
    ) -> Result<Option<u32>, MemoryError> {
        self.reserve(size, Kind::Crt, memory)
    }

    pub(super) fn crt_length(&self, pointer: u32) -> Option<u64> {
        self.allocations
            .get(&pointer)
            .filter(|allocation| matches!(allocation.kind, Kind::Crt))
            .map(|allocation| allocation.length)
    }

    pub(super) fn free_crt(
        &mut self,
        pointer: u32,
        stack: u32,
        memory: &mut GuestMemory,
    ) -> Result<(), DispatchError> {
        if pointer == 0 {
            return Ok(());
        }
        if !self
            .allocations
            .get(&pointer)
            .is_some_and(|allocation| matches!(allocation.kind, Kind::Crt))
        {
            return Err(DispatchError::Unsupported);
        }
        self.release(pointer, stack, memory)
    }

    fn free(
        &mut self,
        global: bool,
        handle: u32,
        stack: u32,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if handle == 0 {
            return Ok(0);
        }
        let valid = self.allocations.get(&handle).is_some_and(|allocation| {
            if global {
                allocation.global()
            } else {
                matches!(allocation.kind, Kind::Local)
            }
        });
        if !valid {
            thread::set_last_error(memory, 6)?;
            return Ok(handle);
        }
        self.release(handle, stack, memory)?;
        Ok(0)
    }

    fn release(
        &mut self,
        handle: u32,
        stack: u32,
        memory: &mut GuestMemory,
    ) -> Result<(), DispatchError> {
        let allocation = &self.allocations[&handle];
        let address = u64::from(allocation.base);
        // dispatch must still read the return slot after the call completes.
        if address < u64::from(stack) + 8 && u64::from(stack) < address + allocation.length {
            return Err(DispatchError::Unsupported);
        }
        memory.unmap(address, allocation.length)?;
        self.allocations.remove(&handle);
        Ok(())
    }

    fn lock(&mut self, handle: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let Some(allocation) = self
            .allocations
            .get_mut(&handle)
            .filter(|allocation| allocation.global())
        else {
            thread::set_last_error(memory, 6)?;
            return Ok(0);
        };
        if let Kind::GlobalMovable { discarded, locks } = &mut allocation.kind {
            if *discarded {
                thread::set_last_error(memory, 157)?;
                return Ok(0);
            }
            *locks = locks.checked_add(1).ok_or(DispatchError::Unsupported)?;
        }
        Ok(allocation.base)
    }

    fn unlock(&mut self, handle: u32, memory: &mut GuestMemory) -> Result<u32, DispatchError> {
        let Some(allocation) = self
            .allocations
            .get_mut(&handle)
            .filter(|allocation| allocation.global())
        else {
            thread::set_last_error(memory, 6)?;
            return Ok(0);
        };
        let Kind::GlobalMovable { locks, .. } = &mut allocation.kind else {
            return Ok(1);
        };
        match *locks {
            0 => {
                thread::set_last_error(memory, 158)?;
                Ok(0)
            }
            1 => {
                thread::set_last_error(memory, 0)?;
                *locks = 0;
                Ok(0)
            }
            _ => {
                *locks -= 1;
                Ok(1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_lock_overflow_preserves_count() {
        let mut heap = Heap::default();
        let mut memory = GuestMemory::new(1);
        let Ok(handle) = heap.allocate(true, 2, 1, &mut memory) else {
            panic!()
        };
        let Kind::GlobalMovable { locks, .. } =
            &mut heap.allocations.get_mut(&handle).unwrap().kind
        else {
            panic!()
        };
        *locks = u32::MAX;
        assert!(matches!(
            heap.lock(handle, &mut memory),
            Err(DispatchError::Unsupported)
        ));
        let Kind::GlobalMovable { locks, .. } = heap.allocations[&handle].kind else {
            panic!()
        };
        assert_eq!(locks, u32::MAX);
    }
}
