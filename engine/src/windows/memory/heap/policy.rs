use super::*;

const SMALL_LIMIT: u32 = 16 * 1024;
const CLASSES: usize = 72;

pub(super) struct SmallBlocks32 {
    free: [u32; CLASSES],
}
impl SmallBlocks32 {
    pub(super) fn new() -> Self {
        Self { free: [0; CLASSES] }
    }
    pub(super) fn extent(size: u32) -> Option<u32> {
        let size = size.max(1);
        if size > SMALL_LIMIT {
            return None;
        }
        let quantum = if size <= 128 {
            8
        } else {
            size.next_power_of_two() / 16
        };
        Some(size.div_ceil(quantum) * quantum)
    }
    fn class(extent: u32) -> Option<usize> {
        if extent == 0 || extent > SMALL_LIMIT || Self::extent(extent)? != extent {
            return None;
        }
        if extent <= 128 {
            Some((extent / 8 - 1) as usize)
        } else {
            let power = (extent - 1).next_power_of_two();
            let band = power.trailing_zeros() - 8;
            Some(16 + band as usize * 8 + ((extent - power / 2) / (power / 16) - 1) as usize)
        }
    }
    pub(super) fn rebuild(&mut self, blocks: &[Block32]) {
        self.free.fill(0);
        for block in blocks.iter().filter(|block| block.size.is_none()) {
            if let Some(class) = Self::class(block.extent)
                && self.free[class] == 0
            {
                self.free[class] = block.address;
            }
        }
    }
    pub(super) fn allocated(&mut self, original: Block32, extent: u32) {
        if let Some(class) = Self::class(original.extent)
            && self.free[class] == original.address
        {
            self.free[class] = 0;
        }
        if let Some(class) = Self::class(original.extent - extent)
            && self.free[class] == 0
        {
            self.free[class] = original.address + extent;
        }
    }
    pub(super) fn find(&self, extent: u32, blocks: &[Block32]) -> Option<usize> {
        let address = self.free[Self::class(extent)?];
        if address == 0 {
            return None;
        }
        blocks
            .binary_search_by_key(&address, |block| block.address)
            .ok()
    }
}
impl HeapManager32 {
    pub(crate) fn compatibility(&self, handle: u32) -> Result<u32, MemoryError32> {
        Ok(if self.heaps[self.index(handle)?].small.is_some() {
            2
        } else {
            0
        })
    }
    pub(crate) fn enable_small_blocks(
        &mut self,
        handle: u32,
        mode: u32,
    ) -> Result<(), MemoryError32> {
        let heap = self.index(handle)?;
        if mode != 2
            || self.heaps[heap].maximum != 0
            || self.heaps[heap].flags & HEAP_NO_SERIALIZE != 0
        {
            return Err(MemoryError32::InvalidParameter);
        }
        if self.heaps[heap].small.is_none() {
            let mut small = SmallBlocks32::new();
            small.rebuild(&self.heaps[heap].blocks);
            self.heaps[heap].small = Some(small);
        }
        Ok(())
    }
    pub(crate) fn enable_termination_on_corruption(&mut self) {
        self.terminate_on_corruption = true;
    }
    pub(super) fn extent(&self, heap: usize, size: u32) -> Result<u32, MemoryError32> {
        if self.heaps[heap].small.is_some()
            && let Some(extent) = SmallBlocks32::extent(size)
        {
            return Ok(extent);
        }
        checked_size(size)
    }
    pub(super) fn record_small_allocation(&mut self, heap: usize, original: Block32, extent: u32) {
        if let Some(small) = self.heaps[heap].small.as_mut() {
            small.allocated(original, extent);
        }
    }
    pub(super) fn rebuild_small_blocks(&mut self, heap: usize) {
        let heap = &mut self.heaps[heap];
        if let Some(small) = heap.small.as_mut() {
            small.rebuild(&heap.blocks);
        }
    }
    pub(super) fn invalid_block(&self, heap: usize, pointer: u32) -> MemoryError32 {
        let owned = self.heaps[heap].blocks.iter().any(|block| {
            pointer >= block.address
                && u64::from(pointer) < u64::from(block.address) + u64::from(block.extent)
        });
        if self.terminate_on_corruption && owned {
            MemoryError32::HeapCorruption
        } else {
            MemoryError32::InvalidParameter
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SmallBlocks32;
    #[test]
    fn every_small_size_has_one_minimal_bounded_bucket_and_large_requests_keep_standard_alignment()
    {
        let mut previous = 0;
        for size in 1..=16384 {
            let extent = SmallBlocks32::extent(size).unwrap();
            assert!(extent >= size && extent.is_multiple_of(8));
            assert!(extent >= previous);
            if extent != previous {
                assert_eq!(size, previous + 1);
            }
            previous = extent;
        }
        assert_eq!(SmallBlocks32::extent(16385), None);
    }
}
