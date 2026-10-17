use super::*;

impl HeapManager32 {
    pub(crate) fn local_allocate(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        external: &[PageRange],
        flags: u32,
        size: u32,
    ) -> Result<u32, MemoryError32> {
        if flags & !(2 | 0x40 | 0x1000 | 0x2000 | 0x10 | 0x20) != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let moving = flags & 2 != 0;
        let next = if moving {
            if self.movable.len() >= self.limits.moveable_handles {
                return Err(MemoryError32::Capacity);
            }
            self.movable
                .try_reserve(1)
                .map_err(|_| MemoryError32::Allocation)?;
            Some(
                self.next_movable
                    .checked_add(4)
                    .ok_or(MemoryError32::Exhausted)?,
            )
        } else {
            None
        };
        let heap = self.ensure_default(vm, memory, external)?;
        let pointer = if moving && size == 0 {
            0
        } else {
            self.allocate(
                vm,
                memory,
                heap,
                if flags & 0x40 != 0 {
                    HEAP_ZERO_MEMORY
                } else {
                    0
                },
                size,
            )?
        };
        if let Some(next) = next {
            let handle = self.next_movable;
            self.next_movable = next;
            self.movable.push(Movable32 {
                handle,
                pointer,
                locks: 0,
                discardable: flags & 0x1000 != 0,
                transferred: false,
            });
            Ok(handle)
        } else {
            Ok(pointer)
        }
    }
    pub(crate) fn local_reallocate(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
        size: u32,
        flags: u32,
    ) -> Result<u32, MemoryError32> {
        if flags & !(2 | 0x40 | 0x80 | 0x1000 | 0x2000 | 0x10 | 0x20) != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.default.ok_or(MemoryError32::InvalidHandle)?;
        let moving = self.movable.iter().position(|block| block.handle == handle);
        if moving.is_some_and(|index| self.movable[index].transferred) {
            return Err(MemoryError32::InvalidHandle);
        }
        if flags & 0x80 != 0 {
            if flags & 0x40 != 0 {
                return Err(MemoryError32::InvalidParameter);
            }
            if let Some(index) = moving {
                self.movable[index].discardable = flags & 0x1000 != 0;
                return Ok(handle);
            }
            self.size(heap, 0, handle)?;
            if flags & 2 == 0 {
                return Ok(handle);
            }
            if self.movable.len() >= self.limits.moveable_handles {
                return Err(MemoryError32::Capacity);
            }
            let next = self
                .next_movable
                .checked_add(4)
                .ok_or(MemoryError32::Exhausted)?;
            self.movable
                .try_reserve(1)
                .map_err(|_| MemoryError32::Allocation)?;
            let movable = self.next_movable;
            self.next_movable = next;
            self.movable.push(Movable32 {
                handle: movable,
                pointer: handle,
                locks: 0,
                discardable: flags & 0x1000 != 0,
                transferred: false,
            });
            return Ok(movable);
        }
        let heap_flags = if flags & 0x40 != 0 {
            HEAP_ZERO_MEMORY
        } else {
            0
        };
        if let Some(index) = moving {
            let pointer = self.movable[index].pointer;
            if size == 0 && flags & 2 != 0 {
                if self.movable[index].locks != 0 {
                    return Err(MemoryError32::Busy);
                }
                if pointer != 0 {
                    self.free(heap, 0, pointer)?;
                }
                self.movable[index].pointer = 0;
                return Ok(handle);
            }
            let next = if pointer == 0 {
                self.allocate(vm, memory, heap, heap_flags, size)?
            } else {
                self.reallocate(
                    vm,
                    memory,
                    heap,
                    heap_flags
                        | if self.movable[index].locks != 0 && flags & 2 == 0 {
                            HEAP_REALLOC_IN_PLACE_ONLY
                        } else {
                            0
                        },
                    pointer,
                    size,
                )?
            };
            self.movable[index].pointer = next;
            Ok(handle)
        } else {
            self.reallocate(
                vm,
                memory,
                heap,
                heap_flags
                    | if flags & 2 == 0 {
                        HEAP_REALLOC_IN_PLACE_ONLY
                    } else {
                        0
                    },
                handle,
                size,
            )
        }
    }
    pub(crate) fn local_free(&mut self, handle: u32) -> Result<(), MemoryError32> {
        if handle == 0 {
            return Ok(());
        }
        let heap = self.default.ok_or(MemoryError32::InvalidHandle)?;
        if let Some(index) = self.movable.iter().position(|block| block.handle == handle) {
            if self.movable[index].transferred {
                return Err(MemoryError32::InvalidHandle);
            }
            let pointer = self.movable[index].pointer;
            if pointer != 0 {
                self.free(heap, 0, pointer)?;
            }
            self.movable.remove(index);
            Ok(())
        } else {
            self.free(heap, 0, handle)
        }
    }
    pub(crate) fn local_lock(&mut self, handle: u32) -> Result<u32, MemoryError32> {
        if let Some(block) = self.movable.iter_mut().find(|block| block.handle == handle) {
            if block.pointer == 0 {
                return Err(MemoryError32::Discarded);
            }
            block.locks = block.locks.checked_add(1).ok_or(MemoryError32::Exhausted)?;
            return Ok(block.pointer);
        }
        self.size(self.default.ok_or(MemoryError32::InvalidHandle)?, 0, handle)?;
        Ok(handle)
    }
    pub(crate) fn local_unlock(
        &mut self,
        handle: u32,
        global: bool,
    ) -> Result<bool, MemoryError32> {
        if let Some(block) = self.movable.iter_mut().find(|block| block.handle == handle) {
            block.locks = block.locks.checked_sub(1).ok_or(MemoryError32::NotLocked)?;
            return Ok(block.locks != 0);
        }
        self.size(self.default.ok_or(MemoryError32::InvalidHandle)?, 0, handle)?;
        if global {
            Ok(true)
        } else {
            Err(MemoryError32::NotLocked)
        }
    }
    pub(crate) fn local_size(&self, handle: u32) -> Result<u32, MemoryError32> {
        let pointer = self
            .movable
            .iter()
            .find(|block| block.handle == handle)
            .map_or(handle, |block| block.pointer);
        if pointer == 0 {
            return Ok(0);
        }
        self.size(
            self.default.ok_or(MemoryError32::InvalidHandle)?,
            0,
            pointer,
        )
    }
    pub(crate) fn local_flags(&self, handle: u32) -> Result<u32, MemoryError32> {
        if let Some(block) = self.movable.iter().find(|block| block.handle == handle) {
            return Ok(u32::from(block.locks.min(255))
                | if block.discardable { 0x1000 } else { 0 }
                | if block.pointer == 0 { 0x4000 } else { 0 });
        }
        self.local_size(handle)?;
        Ok(0)
    }
    pub(crate) fn local_handle(&self, pointer: u32) -> Result<u32, MemoryError32> {
        if let Some(block) = self
            .movable
            .iter()
            .find(|block| block.pointer == pointer && pointer != 0)
        {
            return Ok(block.handle);
        }
        self.size(
            self.default.ok_or(MemoryError32::InvalidHandle)?,
            0,
            pointer,
        )?;
        Ok(pointer)
    }
}
