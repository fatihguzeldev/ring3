use super::*;

impl HeapManager32 {
    pub(crate) fn ensure_default(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        external: &[PageRange],
    ) -> Result<u32, MemoryError32> {
        if let Some(handle) = self.default {
            return Ok(handle);
        }
        let handle = self.create(vm, memory, external, 0, 0, 0)?;
        self.default = Some(handle);
        Ok(handle)
    }
    pub(crate) fn create(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        external: &[PageRange],
        flags: u32,
        initial: u32,
        maximum: u32,
    ) -> Result<u32, MemoryError32> {
        if flags & !(HEAP_NO_SERIALIZE | HEAP_GENERATE_EXCEPTIONS | 0x40000) != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        if self.heaps.len() >= self.limits.heaps {
            return Err(MemoryError32::Capacity);
        }
        let reserve = if maximum == 0 {
            self.limits
                .heap_reserve_bytes
                .max(initial.checked_add(65535).ok_or(MemoryError32::Capacity)? / 65536 * 65536)
        } else {
            maximum
                .checked_add(PAGE_SIZE - 1)
                .ok_or(MemoryError32::Capacity)?
                / PAGE_SIZE
                * PAGE_SIZE
        };
        if initial > reserve || reserve == 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        self.heaps
            .try_reserve(1)
            .map_err(|_| MemoryError32::Allocation)?;
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(1)
            .map_err(|_| MemoryError32::Allocation)?;
        let handle = vm.allocate(
            memory,
            external,
            0,
            reserve,
            MEM_RESERVE,
            if flags & 0x40000 != 0 { 0x40 } else { 4 },
        )?;
        // initial commit is prepared before the reservation is exposed as a heap handle.
        if initial != 0
            && let Err(error) = vm.allocate(
                memory,
                external,
                handle,
                initial,
                MEM_COMMIT,
                if flags & 0x40000 != 0 { 0x40 } else { 4 },
            )
        {
            vm.free(memory, handle, 0, MEM_RELEASE)
                .expect("uncommitted heap reservation rollback cannot fail");
            return Err(error);
        }
        blocks.push(Block32 {
            address: handle,
            extent: reserve,
            size: None,
            allocation: 0,
        });
        self.heaps.push(Heap32 {
            handle,
            maximum,
            flags,
            blocks,
            locks: 0,
            small: None,
        });
        Ok(handle)
    }
    pub(crate) fn destroy(
        &mut self,
        vm: &mut VirtualMemory32,
        memory: &mut AddressSpace,
        handle: u32,
    ) -> Result<(), MemoryError32> {
        if self.default == Some(handle) {
            return Err(MemoryError32::InvalidHandle);
        }
        let index = self.index(handle)?;
        vm.free(memory, handle, 0, MEM_RELEASE)?;
        self.heaps.remove(index);
        self.movable.retain(|block| {
            block.pointer == 0
                || self.heaps.iter().any(|heap| {
                    heap.blocks
                        .iter()
                        .any(|item| item.address == block.pointer && item.size.is_some())
                })
        });
        Ok(())
    }
    pub(crate) fn free(
        &mut self,
        handle: u32,
        flags: u32,
        pointer: u32,
    ) -> Result<(), MemoryError32> {
        if flags & !HEAP_NO_SERIALIZE != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.index(handle)?;
        if pointer == 0 {
            return Ok(());
        }
        let block = self.block(heap, pointer)?;
        self.heaps[heap].blocks[block].size = None;
        self.heaps[heap].blocks[block].allocation = 0;
        self.coalesce_at(heap, block);
        Ok(())
    }
}
