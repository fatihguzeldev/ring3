use super::*;

pub(super) struct FreeBlocks32 {
    blocks: Vec<(u32, u32)>,
}

impl FreeBlocks32 {
    pub(super) fn new(blocks: &[Block32]) -> Result<Self, MemoryError32> {
        let mut free = Self { blocks: Vec::new() };
        free.reserve_for(blocks.len())?;
        free.rebuild(blocks);
        Ok(free)
    }

    pub(super) fn reserve_for(&mut self, block_count: usize) -> Result<(), MemoryError32> {
        if self.blocks.capacity() < block_count {
            self.blocks
                .try_reserve(block_count - self.blocks.len())
                .map_err(|_| MemoryError32::Allocation)?;
        }
        Ok(())
    }

    pub(super) fn find(&self, extent: u32) -> Option<u32> {
        self.blocks
            .get(self.blocks.partition_point(|&(size, _)| size < extent))
            .map(|&(_, address)| address)
    }

    pub(super) fn exact(&self, extent: u32) -> Option<u32> {
        self.blocks
            .get(self.blocks.partition_point(|&(size, _)| size < extent))
            .filter(|&&(size, _)| size == extent)
            .map(|&(_, address)| address)
    }

    pub(super) fn remove(&mut self, block: Block32) {
        let index = self
            .blocks
            .binary_search(&(block.extent, block.address))
            .expect("free block metadata remains indexed");
        self.blocks.remove(index);
    }

    pub(super) fn insert(&mut self, block: Block32) {
        debug_assert!(block.size.is_none());
        let index = self
            .blocks
            .binary_search(&(block.extent, block.address))
            .expect_err("free block is inserted once");
        assert!(self.blocks.len() < self.blocks.capacity());
        self.blocks.insert(index, (block.extent, block.address));
    }

    pub(super) fn rebuild(&mut self, blocks: &[Block32]) {
        assert!(self.blocks.capacity() >= blocks.len());
        self.blocks.clear();
        self.blocks.extend(
            blocks
                .iter()
                .filter(|block| block.size.is_none())
                .map(|block| (block.extent, block.address)),
        );
        self.blocks.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_fit_matches_the_extent_then_address_scan_for_fragmented_blocks() {
        let mut blocks = Vec::new();
        let mut address = 0x10000;
        for index in 0..4096 {
            let extent = (index * 37 % 127 + 1) * 8;
            blocks.push(Block32 {
                address,
                extent,
                size: (index % 3 == 0).then_some(extent),
                allocation: u64::from(index + 1),
            });
            address += extent;
        }
        let free = FreeBlocks32::new(&blocks).unwrap();
        for extent in (8..=2048).step_by(8) {
            let expected = blocks
                .iter()
                .filter(|block| block.size.is_none() && block.extent >= extent)
                .min_by_key(|block| block.extent)
                .map(|block| block.address);
            assert_eq!(free.find(extent), expected);
        }
    }

    #[test]
    fn equal_extents_choose_the_lowest_address_after_arbitrary_retirement_order() {
        let blocks = [
            Block32 {
                address: 0x10000,
                extent: 32,
                size: Some(32),
                allocation: 1,
            },
            Block32 {
                address: 0x10020,
                extent: 16,
                size: Some(16),
                allocation: 2,
            },
            Block32 {
                address: 0x10030,
                extent: 32,
                size: Some(32),
                allocation: 3,
            },
        ];
        let mut free = FreeBlocks32::new(&blocks).unwrap();
        for index in [2, 0, 1] {
            free.insert(Block32 {
                size: None,
                allocation: 0,
                ..blocks[index]
            });
        }
        assert_eq!(free.find(8), Some(0x10020));
        assert_eq!(free.find(24), Some(0x10000));
        assert_eq!(free.exact(32), Some(0x10000));
        free.remove(Block32 {
            size: None,
            allocation: 0,
            ..blocks[0]
        });
        assert_eq!(free.find(24), Some(0x10030));
    }

    fn assert_index(heaps: &HeapManager32, handle: u32) {
        let heap = &heaps.heaps[heaps.index(handle).unwrap()];
        let mut expected: Vec<_> = heap
            .blocks
            .iter()
            .filter(|block| block.size.is_none())
            .map(|block| (block.extent, block.address))
            .collect();
        expected.sort_unstable();
        assert_eq!(heap.free.blocks, expected);
        assert!(heap.free.blocks.capacity() >= heap.blocks.len());
    }

    #[test]
    fn index_survives_single_and_batch_mutations_and_failed_memory_publication() {
        for small in [false, true] {
            let mut memory = AddressSpace::new(64).unwrap();
            let mut vm = VirtualMemory32::new(MemoryLimits32::default()).unwrap();
            let mut heaps = HeapManager32::new(MemoryLimits32::default()).unwrap();
            let heap = heaps.ensure_default(&mut vm, &mut memory, &[]).unwrap();
            if small {
                heaps.enable_small_blocks(heap, 2).unwrap();
            }
            let mut live = Vec::new();
            for step in 0..140 {
                let size = (step * 37 % 511 + 1) as u32;
                let index = heaps.index(heap).unwrap();
                match if live.is_empty() { 0 } else { step % 7 } {
                    0 | 1 => {
                        let extent = heaps.extent(index, size).unwrap();
                        let source = &heaps.heaps[index];
                        let expected = source
                            .small
                            .as_ref()
                            .and_then(|small| small.find(extent, &source.blocks))
                            .map(|block| source.blocks[block].address)
                            .or_else(|| {
                                source
                                    .blocks
                                    .iter()
                                    .filter(|block| block.size.is_none() && block.extent >= extent)
                                    .min_by_key(|block| block.extent)
                                    .map(|block| block.address)
                            })
                            .unwrap();
                        let pointer = heaps.allocate(&mut vm, &mut memory, heap, 0, size).unwrap();
                        assert_eq!(pointer, expected);
                        live.push(pointer);
                    }
                    2 => heaps.free(heap, 0, live.remove(0)).unwrap(),
                    3 => {
                        let before = live[0];
                        memory.write(GuestAddress(before), &[0x5a]).unwrap();
                        live[0] = heaps
                            .reallocate(&mut vm, &mut memory, heap, 0, before, size)
                            .unwrap();
                        let mut first = [0];
                        memory.read(GuestAddress(live[0]), &mut first).unwrap();
                        assert_eq!(first, [0x5a]);
                    }
                    4 => {
                        let retired = live.remove(0);
                        let prepared = heaps
                            .prepare_initialized_allocations(&memory, heap, &[13, 47], &[retired])
                            .unwrap();
                        let addresses = prepared.addresses().to_vec();
                        heaps
                            .commit_initialized_allocations(
                                &mut vm,
                                &mut memory,
                                prepared,
                                &[&[1; 13], &[2; 47]],
                                &[],
                            )
                            .unwrap();
                        live.extend(addresses);
                    }
                    5 => {
                        let retired = live.remove(0);
                        let allocation = heaps.allocation_identity(heap, retired).unwrap();
                        let prepared = heaps
                            .prepare_retirement(heap, &[(retired, allocation)])
                            .unwrap();
                        heaps.commit_retirement_after_memory(prepared);
                    }
                    6 => {
                        let prepared = heaps
                            .prepare_initialized_allocation(&memory, heap, size)
                            .unwrap();
                        let version = memory.transaction_version();
                        assert_eq!(
                            heaps.commit_initialized_allocation(
                                &mut vm,
                                &mut memory,
                                prepared,
                                &[],
                                &[(GuestAddress(0xfffffff0), &[0; 4])],
                            ),
                            Err(MemoryError32::NoAccess)
                        );
                        assert_eq!(memory.transaction_version(), version);
                    }
                    _ => unreachable!(),
                }
                assert_index(&heaps, heap);
                assert!(heaps.validate(heap, 0, 0).unwrap());
            }
        }
    }
}
