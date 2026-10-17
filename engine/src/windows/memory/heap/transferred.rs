use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GlobalMemory32 {
    pub handle: u32,
    pub pointer: u32,
    pub allocation: u64,
    pub bytes: u32,
}

pub(crate) struct PreparedGlobalMemory32 {
    allocation: PreparedHeapAllocation32,
    handle: u32,
}

pub(crate) struct PreparedGlobalRetirement32 {
    heap: PreparedHeapRetirement32,
    handles: Vec<u32>,
}

impl HeapManager32 {
    pub(crate) fn global_memory(&self, handle: u32) -> Result<GlobalMemory32, MemoryError32> {
        let movable = self
            .movable
            .iter()
            .find(|block| block.handle == handle)
            .ok_or(MemoryError32::InvalidHandle)?;
        if movable.pointer == 0 {
            return Err(MemoryError32::Discarded);
        }
        let heap = self.default.ok_or(MemoryError32::InvalidHandle)?;
        Ok(GlobalMemory32 {
            handle,
            pointer: movable.pointer,
            allocation: self.allocation_identity(heap, movable.pointer)?,
            bytes: self.size(heap, 0, movable.pointer)?,
        })
    }

    pub(crate) fn check_global_memory(&self, memory: GlobalMemory32) -> Result<(), MemoryError32> {
        if self.global_memory(memory.handle)? != memory {
            return Err(MemoryError32::InvalidHandle);
        }
        Ok(())
    }

    pub(crate) fn commit_global_transfer(&mut self, memory: GlobalMemory32) {
        self.movable
            .iter_mut()
            .find(|block| block.handle == memory.handle)
            .expect("global memory identity was preflighted")
            .transferred = true;
    }

    pub(crate) fn prepare_transferred_global(
        &mut self,
        memory: &AddressSpace,
        bytes: u32,
    ) -> Result<PreparedGlobalMemory32, MemoryError32> {
        if self.movable.len() == self.limits.moveable_handles {
            return Err(MemoryError32::Capacity);
        }
        self.next_movable
            .checked_add(4)
            .ok_or(MemoryError32::Exhausted)?;
        self.movable
            .try_reserve(1)
            .map_err(|_| MemoryError32::Allocation)?;
        let allocation = self.prepare_initialized_allocation(
            memory,
            self.default.ok_or(MemoryError32::InvalidHandle)?,
            bytes,
        )?;
        Ok(PreparedGlobalMemory32 {
            allocation,
            handle: self.next_movable,
        })
    }

    pub(crate) fn commit_transferred_global(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        prepared: PreparedGlobalMemory32,
        bytes: &[u8],
    ) -> Result<GlobalMemory32, MemoryError32> {
        if self.next_movable != prepared.handle {
            return Err(MemoryError32::InvalidHandle);
        }
        let pointer =
            self.commit_initialized_allocation(vm, memory, prepared.allocation, bytes, &[])?;
        self.next_movable += 4;
        self.movable.push(Movable32 {
            handle: prepared.handle,
            pointer,
            locks: 0,
            discardable: false,
            transferred: true,
        });
        Ok(self
            .global_memory(prepared.handle)
            .expect("published global allocation retains its exact identity"))
    }

    pub(crate) fn prepare_global_retirement(
        &self,
        owned: &[GlobalMemory32],
    ) -> Result<PreparedGlobalRetirement32, MemoryError32> {
        let mut handles = Vec::new();
        let mut blocks = Vec::new();
        handles
            .try_reserve_exact(owned.len())
            .map_err(|_| MemoryError32::Allocation)?;
        blocks
            .try_reserve_exact(owned.len())
            .map_err(|_| MemoryError32::Allocation)?;
        for &memory in owned {
            self.check_global_memory(memory)?;
            if !self
                .movable
                .iter()
                .any(|row| row.handle == memory.handle && row.transferred)
            {
                return Err(MemoryError32::InvalidHandle);
            }
            if !handles.contains(&memory.handle) {
                handles.push(memory.handle);
                blocks.push((memory.pointer, memory.allocation));
            }
        }
        handles.sort_unstable();
        let heap =
            self.prepare_retirement(self.default.ok_or(MemoryError32::InvalidHandle)?, &blocks)?;
        Ok(PreparedGlobalRetirement32 { heap, handles })
    }

    pub(crate) fn commit_global_retirement(&mut self, prepared: PreparedGlobalRetirement32) {
        self.commit_retirement_after_memory(prepared.heap);
        self.movable
            .retain(|row| prepared.handles.binary_search(&row.handle).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transferred_globals_remain_readable_until_their_canonical_owner_retires_them() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let handle = heaps
            .local_allocate(&mut vm, &mut memory, &[], 2, 8)
            .unwrap();
        let owned = heaps.global_memory(handle).unwrap();
        heaps.check_global_memory(owned).unwrap();
        heaps.commit_global_transfer(owned);
        assert_eq!(heaps.local_free(handle), Err(MemoryError32::InvalidHandle));
        assert_eq!(
            heaps.local_reallocate(&mut vm, &mut memory, handle, 16, 2),
            Err(MemoryError32::InvalidHandle)
        );
        assert_eq!(heaps.local_lock(handle).unwrap(), owned.pointer);
        assert!(!heaps.local_unlock(handle, true).unwrap());
        let prepared = heaps.prepare_global_retirement(&[owned, owned]).unwrap();
        let version = memory.transaction_version();
        heaps.commit_global_retirement(prepared);
        assert_eq!(memory.transaction_version(), version);
        assert!(heaps.global_memory(handle).is_err());
    }

    #[test]
    fn initialized_provider_global_publishes_only_after_the_payload_is_accepted() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let next = heaps.next_movable;
        let prepared = heaps.prepare_transferred_global(&memory, 8).unwrap();
        assert!(
            heaps
                .commit_transferred_global(&mut vm, &mut memory, prepared, &[0; 9])
                .is_err()
        );
        assert_eq!(heaps.next_movable, next);
        assert!(heaps.global_memory(next).is_err());
        let prepared = heaps.prepare_transferred_global(&memory, 8).unwrap();
        let owned = heaps
            .commit_transferred_global(&mut vm, &mut memory, prepared, b"clip\0")
            .unwrap();
        assert_eq!(owned.handle, next);
        assert_eq!(owned.bytes, 8);
        let mut bytes = [0; 5];
        memory
            .read(GuestAddress(owned.pointer), &mut bytes)
            .unwrap();
        assert_eq!(&bytes, b"clip\0");
    }
}
