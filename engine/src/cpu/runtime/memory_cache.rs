pub(crate) const ROW_COUNT: usize = 1 << 20;
pub(crate) const RESIDENT_BYTES: usize =
    std::mem::size_of::<MemoryCache>() + ROW_COUNT * std::mem::size_of::<u64>();

#[repr(C, align(16))]
pub(crate) struct MemoryCache {
    generation: u32,
    identity: [u32; 3],
    pointer: u32,
    count: u32,
    reserved: [u32; 2],
    entries: Box<[u64]>,
}

impl MemoryCache {
    pub(crate) fn new() -> Result<Self, std::collections::TryReserveError> {
        let mut entries = Vec::new();
        entries.try_reserve_exact(ROW_COUNT)?;
        entries.resize(ROW_COUNT, 0);
        let entries = entries.into_boxed_slice();
        Ok(Self {
            generation: 1,
            identity: [0x434d_3352, 32, 2],
            pointer: entries.as_ptr() as usize as u32,
            count: ROW_COUNT as u32,
            reserved: [0; 2],
            entries,
        })
    }

    pub(crate) fn invalidate(&mut self) {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            self.entries.fill(0);
            self.generation = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_page_cache_invalidates_all_proofs_and_clears_generation_wrap() {
        assert_eq!(std::mem::offset_of!(MemoryCache, pointer), 16);
        assert_eq!(std::mem::offset_of!(MemoryCache, entries), 32);
        let mut cache = MemoryCache::new().unwrap();
        assert_eq!(cache.identity, [0x434d_3352, 32, 2]);
        assert_eq!(cache.pointer, cache.entries.as_ptr() as usize as u32);
        assert_eq!(cache.count, ROW_COUNT as u32);
        assert_eq!(cache.reserved, [0; 2]);
        assert_eq!(
            RESIDENT_BYTES,
            std::mem::size_of::<MemoryCache>() + cache.entries.len() * 8
        );
        cache.entries[0] = 1 << 32 | 7;
        cache.entries[ROW_COUNT - 1] = 1 << 32 | 0xffff_f007;
        cache.invalidate();
        assert_eq!(cache.generation, 2);
        assert_eq!(cache.entries[0] >> 32, 1);
        assert_eq!(cache.entries[ROW_COUNT - 1] >> 32, 1);
        cache.generation = u32::MAX;
        cache.entries[19] = u64::MAX;
        cache.invalidate();
        assert_eq!(cache.generation, 1);
        assert!(cache.entries.iter().all(|entry| *entry == 0));
    }
}
