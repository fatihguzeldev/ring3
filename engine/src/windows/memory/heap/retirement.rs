use super::*;

pub(crate) struct PreparedHeapRetirement32 {
    heap: usize,
    handle: u32,
    blocks: Vec<Block32>,
    free: free::FreeBlocks32,
}

impl HeapManager32 {
    pub(crate) fn prepare_retirement(
        &self,
        handle: u32,
        owned: &[(u32, u64)],
    ) -> Result<PreparedHeapRetirement32, MemoryError32> {
        let heap = self.index(handle)?;
        let source = &self.heaps[heap].blocks;
        if owned.len() > source.len() {
            return Err(MemoryError32::InvalidParameter);
        }
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(source.len())
            .map_err(|_| MemoryError32::Allocation)?;
        blocks.extend_from_slice(source);
        for &(pointer, allocation) in owned {
            let block = self.block(heap, pointer)?;
            if allocation == 0
                || blocks[block].size.is_none()
                || blocks[block].allocation != allocation
            {
                return Err(MemoryError32::InvalidParameter);
            }
            blocks[block].size = None;
            blocks[block].allocation = 0;
        }
        group::coalesce_blocks(&mut blocks);
        let free = free::FreeBlocks32::new(&blocks)?;
        Ok(PreparedHeapRetirement32 {
            heap,
            handle,
            blocks,
            free,
        })
    }

    // the caller publishes only RAM changes between preparation and this metadata commit.
    pub(crate) fn commit_retirement_after_memory(&mut self, prepared: PreparedHeapRetirement32) {
        assert_eq!(self.heaps[prepared.heap].handle, prepared.handle);
        self.heaps[prepared.heap].blocks = prepared.blocks;
        self.heaps[prepared.heap].free = prepared.free;
        self.rebuild_small_blocks(prepared.heap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_preserves_neighbors_and_does_not_publish_ram_or_consume_an_identity() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let first = heaps
            .allocate_initialized(&mut vm, &mut memory, heap, b"owned")
            .unwrap();
        let neighbor = heaps
            .allocate_initialized(&mut vm, &mut memory, heap, b"caller")
            .unwrap();
        let first_id = heaps.allocation_identity(heap, first).unwrap();
        let neighbor_id = heaps.allocation_identity(heap, neighbor).unwrap();
        let prepared = heaps
            .prepare_retirement(heap, &[(first, first_id)])
            .unwrap();
        let version = memory.transaction_version();
        let counter = heaps.next_allocation;
        heaps.commit_retirement_after_memory(prepared);
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(heaps.next_allocation, counter);
        assert!(heaps.allocation_identity(heap, first).is_err());
        assert_eq!(
            heaps.allocation_identity(heap, neighbor).unwrap(),
            neighbor_id
        );
        let mut bytes = [0; 6];
        memory.read(GuestAddress(neighbor), &mut bytes).unwrap();
        assert_eq!(&bytes, b"caller");
        assert_eq!(
            heaps.allocate(&mut vm, &mut memory, heap, 0, 5).unwrap(),
            first
        );
        assert_ne!(heaps.allocation_identity(heap, first).unwrap(), first_id);
    }

    #[test]
    fn duplicate_or_reused_ownership_rejects_the_whole_retirement_before_publication() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let pointer = heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap();
        let old = heaps.allocation_identity(heap, pointer).unwrap();
        assert!(
            heaps
                .prepare_retirement(heap, &[(pointer, old), (pointer, old)])
                .is_err()
        );
        assert_eq!(heaps.allocation_identity(heap, pointer).unwrap(), old);
        heaps.free(heap, 0, pointer).unwrap();
        assert_eq!(
            heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap(),
            pointer
        );
        let current = heaps.allocation_identity(heap, pointer).unwrap();
        assert!(heaps.prepare_retirement(heap, &[(pointer, old)]).is_err());
        assert_eq!(heaps.allocation_identity(heap, pointer).unwrap(), current);
    }
}
