use super::*;

impl HeapManager32 {
    pub(super) fn coalesce(&mut self, heap: usize) {
        group::coalesce_blocks(&mut self.heaps[heap].blocks);
        self.rebuild_small_blocks(heap);
    }

    pub(super) fn coalesce_at(&mut self, heap: usize, block: usize) {
        let blocks = &mut self.heaps[heap].blocks;
        debug_assert!(blocks[block].size.is_none());
        let first = if block > 0 && blocks[block - 1].size.is_none() {
            block - 1
        } else {
            block
        };
        let last = if block + 1 < blocks.len() && blocks[block + 1].size.is_none() {
            block + 1
        } else {
            block
        };
        for next in first + 1..=last {
            blocks[first].extent += blocks[next].extent;
        }
        if first != last {
            blocks.drain(first + 1..=last);
        }
        self.rebuild_small_blocks(heap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_retirement_merges_both_neighbors_and_preserves_other_allocation_identities() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let mut pointers = Vec::new();
        for _ in 0..7 {
            pointers.push(heaps.allocate(&mut vm, &mut memory, heap, 0, 8).unwrap());
        }
        let survivor = heaps.allocation_identity(heap, pointers[5]).unwrap();
        heaps.free(heap, 0, pointers[1]).unwrap();
        heaps.free(heap, 0, pointers[3]).unwrap();
        heaps.free(heap, 0, pointers[2]).unwrap();
        let index = heaps.index(heap).unwrap();
        let merged = heaps.heaps[index]
            .blocks
            .iter()
            .find(|block| block.address == pointers[1])
            .unwrap();
        assert_eq!(merged.extent, 24);
        assert!(merged.size.is_none());
        assert_eq!(
            heaps.allocation_identity(heap, pointers[5]).unwrap(),
            survivor
        );
        assert_eq!(
            heaps.allocate(&mut vm, &mut memory, heap, 0, 24).unwrap(),
            pointers[1]
        );
        assert!(heaps.validate(heap, 0, 0).unwrap());
    }

    #[test]
    fn shrink_and_grow_reallocation_leave_no_adjacent_free_blocks() {
        let mut memory = AddressSpace::new(64).unwrap();
        let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
        let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
        let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
        let first = heaps.allocate(&mut vm, &mut memory, heap, 0, 64).unwrap();
        let second = heaps.allocate(&mut vm, &mut memory, heap, 0, 32).unwrap();
        heaps.free(heap, 0, second).unwrap();
        for size in [16, 80, 8, 128] {
            assert_eq!(
                heaps
                    .reallocate(&mut vm, &mut memory, heap, 0, first, size)
                    .unwrap(),
                first
            );
            let blocks = &heaps.heaps[heaps.index(heap).unwrap()].blocks;
            assert!(
                blocks
                    .windows(2)
                    .all(|pair| pair[0].size.is_some() || pair[1].size.is_some())
            );
            assert!(heaps.validate(heap, 0, 0).unwrap());
        }
    }
}
