use super::*;
use crate::{memory::GuestAddress, process::EngineInstance};
impl EngineInstance {
    pub(super) fn global_memory_status(&mut self, output: u32) -> Result<u32, MemoryError32> {
        let mut ranges = self
            .guest_address_reservations()
            .map_err(|_| MemoryError32::Allocation)?;
        ranges.sort_unstable_by_key(|range| range.first);
        let memory = self.memory.as_mut().unwrap();
        let capacity = (u64::from(memory.capacity_pages()) * u64::from(crate::memory::PAGE_SIZE))
            .min(u64::from(u32::MAX)) as u32;
        let used = (u64::from(memory.mapped_pages()) * u64::from(crate::memory::PAGE_SIZE))
            .min(u64::from(u32::MAX)) as u32;
        let free = capacity.saturating_sub(used);
        let first = 0x10000usize / crate::memory::PAGE_SIZE as usize;
        let end = 0x7fff0000usize / crate::memory::PAGE_SIZE as usize;
        let virtual_size = ((end - first) * crate::memory::PAGE_SIZE as usize) as u32;
        let mut cursor = first;
        let mut virtual_pages = 0;
        for range in ranges {
            let start = range.first.clamp(first, end);
            let stop = (range.first + range.count).clamp(first, end);
            if cursor < start {
                virtual_pages += memory.unmapped_pages_in(crate::memory::PageRange {
                    first: cursor,
                    count: start - cursor,
                });
            }
            cursor = cursor.max(stop);
        }
        if cursor < end {
            virtual_pages += memory.unmapped_pages_in(crate::memory::PageRange {
                first: cursor,
                count: end - cursor,
            });
        }
        let virtual_free = virtual_pages * crate::memory::PAGE_SIZE;
        let words = [
            32,
            (u64::from(used) * 100 / u64::from(capacity.max(1))) as u32,
            capacity,
            free,
            capacity,
            free,
            virtual_size,
            virtual_free,
        ];
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        memory.write(GuestAddress(output), &bytes)?;
        Ok(0)
    }
}
