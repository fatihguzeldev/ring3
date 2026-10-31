#[repr(C, align(16))]
pub(crate) struct MemoryCache {
    generation: u32,
    identity: [u32; 3],
    entries: [[u32; 4]; 256],
}

impl Default for MemoryCache {
    fn default() -> Self {
        Self {
            generation: 1,
            identity: [0x434d_3352, 4112, 1],
            entries: [[0; 4]; 256],
        }
    }
}

impl MemoryCache {
    pub(crate) fn invalidate(&mut self) {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            self.entries.fill([0; 4]);
            self.generation = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_page_cache_invalidates_all_proofs_and_clears_generation_wrap() {
        assert_eq!(std::mem::size_of::<MemoryCache>(), 4112);
        assert_eq!(std::mem::offset_of!(MemoryCache, entries), 16);
        let mut cache = MemoryCache::default();
        assert_eq!(cache.identity, [0x434d_3352, 4112, 1]);
        cache.entries[0] = [1, 0, 0x1000, 15];
        cache.entries[255] = [1, 0xfffff, 0x2000, 23];
        cache.invalidate();
        assert_eq!(cache.generation, 2);
        assert_eq!(cache.entries[0][0], 1);
        assert_eq!(cache.entries[255][0], 1);
        cache.generation = u32::MAX;
        cache.entries[19] = [u32::MAX, 19, 0x3000, 31];
        cache.invalidate();
        assert_eq!(cache.generation, 1);
        assert!(cache.entries.iter().all(|entry| *entry == [0; 4]));
    }
}
