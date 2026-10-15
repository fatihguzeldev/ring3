use super::*;

impl HeapManager32 {
    pub(crate) fn size(&self, handle: u32, flags: u32, pointer: u32) -> Result<u32, MemoryError32> {
        if flags & !HEAP_NO_SERIALIZE != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.index(handle)?;
        let block = self.block(heap, pointer)?;
        Ok(self.heaps[heap].blocks[block].size.unwrap())
    }
    pub(crate) fn validate(
        &self,
        handle: u32,
        flags: u32,
        pointer: u32,
    ) -> Result<bool, MemoryError32> {
        if flags & !HEAP_NO_SERIALIZE != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.index(handle)?;
        if pointer != 0 {
            return Ok(self.block(heap, pointer).is_ok());
        }
        let mut next = handle;
        Ok(self.heaps[heap].blocks.iter().all(|block| {
            let valid = block.address == next
                && block.extent != 0
                && block.extent.is_multiple_of(8)
                && block.size.is_none_or(|size| size <= block.extent);
            next = block.address.saturating_add(block.extent);
            valid
        }))
    }
    pub(crate) fn lock(&mut self, handle: u32, unlock: bool) -> Result<(), MemoryError32> {
        let heap = self.index(handle)?;
        if unlock {
            self.heaps[heap].locks = self.heaps[heap]
                .locks
                .checked_sub(1)
                .ok_or(MemoryError32::InvalidParameter)?;
        } else {
            self.heaps[heap].locks = self.heaps[heap]
                .locks
                .checked_add(1)
                .ok_or(MemoryError32::Exhausted)?;
        }
        Ok(())
    }
    pub(crate) fn compact(
        &mut self,
        vm: &VirtualMemory32,
        memory: &AddressSpace,
        handle: u32,
        flags: u32,
    ) -> Result<u32, MemoryError32> {
        if flags & !HEAP_NO_SERIALIZE != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let heap = self.index(handle)?;
        self.coalesce(heap);
        let mut maximum = 0;
        for block in self.heaps[heap]
            .blocks
            .iter()
            .filter(|block| block.size.is_none())
        {
            let mut at = block.address;
            while at < block.address + block.extent {
                let info = vm.query(memory, at)?;
                let size = info.size.min(block.address + block.extent - at);
                if info.state == MEM_COMMIT {
                    maximum = maximum.max(size);
                }
                at += size;
            }
        }
        Ok(maximum)
    }
    pub(crate) fn walk(
        &self,
        vm: &VirtualMemory32,
        memory: &AddressSpace,
        handle: u32,
        previous: u32,
    ) -> Result<[u8; 28], MemoryError32> {
        let heap = self.index(handle)?;
        let mut after = previous == 0;
        for block in &self.heaps[heap].blocks {
            let mut at = block.address;
            while at < block.address + block.extent {
                let info = vm.query(memory, at)?;
                let length = if let Some(size) = block.size {
                    size.max(1)
                } else {
                    info.size.min(block.address + block.extent - at)
                };
                if after {
                    let mut bytes = [0; 28];
                    bytes[..4].copy_from_slice(&at.to_le_bytes());
                    bytes[4..8].copy_from_slice(&length.to_le_bytes());
                    let flags: u16 = if block.size.is_some() {
                        4
                    } else if info.state == MEM_RESERVE {
                        2
                    } else {
                        0
                    };
                    bytes[10..12].copy_from_slice(&flags.to_le_bytes());
                    return Ok(bytes);
                }
                after = at == previous;
                if block.size.is_some() {
                    break;
                }
                at += length;
            }
        }
        Err(MemoryError32::NoMoreItems)
    }
}
