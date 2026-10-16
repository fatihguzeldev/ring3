use super::*;
impl VirtualMemory32 {
    pub(crate) fn commit_heap_ranges(
        &mut self,
        memory: &mut AddressSpace,
        allocation: u32,
        ranges: &[(u32, u32)],
        protect: u32,
        writes: &[(GuestAddress, &[u8])],
    ) -> Result<(), MemoryError32> {
        let permissions = protection(protect)?;
        let index = self
            .region(allocation)
            .ok_or(MemoryError32::InvalidAddress)?;
        if start(self.regions[index].range) != allocation {
            return Err(MemoryError32::InvalidAddress);
        }
        let mut requested = Vec::new();
        requested
            .try_reserve_exact(ranges.len())
            .map_err(|_| MemoryError32::Allocation)?;
        for &(address, size) in ranges {
            let range = span(address, size)?;
            if !contains(self.regions[index].range, range) {
                return Err(MemoryError32::InvalidAddress);
            }
            requested.push(range);
        }
        requested.sort_unstable_by_key(|range| range.first);
        let mut count = 0usize;
        for position in 0..requested.len() {
            let range = requested[position];
            if count != 0 && requested[count - 1].first + requested[count - 1].count >= range.first
            {
                let previous = &mut requested[count - 1];
                previous.count = (previous.first + previous.count).max(range.first + range.count)
                    - previous.first;
            } else {
                requested[count] = range;
                count += 1;
            }
        }
        requested.truncate(count);
        let missing = requested
            .iter()
            .flat_map(|range| range.indices())
            .filter(|&page| !self.committed(index, page))
            .count();
        if missing
            > memory
                .capacity_pages()
                .saturating_sub(memory.mapped_pages()) as usize
        {
            return Err(MemoryError32::Capacity);
        }
        if missing == 0 {
            memory.apply_private_pages(&[], writes)?;
            return Ok(());
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(missing)
            .map_err(|_| MemoryError32::Allocation)?;
        let mut committed = self.clone_segments(index, missing)?;
        for range in requested {
            for page in range.indices().filter(|&page| !self.committed(index, page)) {
                changes.push(PrivatePageChange32::Commit {
                    address: GuestAddress(page as u32 * PAGE_SIZE),
                    permissions,
                });
                committed.push(Committed32 {
                    range: PageRange {
                        first: page,
                        count: 1,
                    },
                    protection: protect,
                });
            }
        }
        normalize(&mut committed);
        self.check_fragments(committed.len(), self.regions[index].committed.len())?;
        memory.apply_private_pages(&changes, writes)?;
        self.regions[index].committed = committed;
        Ok(())
    }
}
