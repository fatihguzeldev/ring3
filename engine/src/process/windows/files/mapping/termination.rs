use super::{context::*, views::memory_error};
use crate::{
    files::{FileError32, FileReadRequest32},
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange, WordWrite32},
    process::windows::files::FileRuntime32,
};

pub(in crate::process::windows::files) struct PreparedMappingExit32 {
    version: u64,
    ranges: Vec<PageRange>,
    words: Vec<WordWrite32>,
}
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn flush_exit_mappings(
        &mut self,
        memory: &AddressSpace,
    ) -> Result<Option<FileReadRequest32>, FileError32> {
        for index in 0..self.mappings.objects.len() {
            let key = self.mappings.objects[index].key;
            if self.mappings.objects[..index]
                .iter()
                .any(|object| object.key == key)
            {
                continue;
            }
            if let Some(request) = self.flush_mapping_pages(memory, key, None)? {
                return Ok(Some(request));
            }
        }
        Ok(None)
    }
}
impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn prepare_process_exit(
        &self,
        memory: &AddressSpace,
        extra_unmap: &[PageRange],
        words: &[WordWrite32],
    ) -> Result<PreparedMappingExit32, FileError32> {
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(self.views.len() + extra_unmap.len())
            .map_err(|_| FileError32::Allocation)?;
        ranges.extend(self.views.iter().map(|view| view.range));
        ranges.extend_from_slice(extra_unmap);
        ranges.sort_unstable_by_key(|range| range.first);
        let mut previous_end = 0;
        for range in &ranges {
            let end = range
                .first
                .checked_add(range.count)
                .ok_or(FileError32::InvalidParameter)?;
            if range.count == 0
                || range.first < previous_end
                || end as u64 * u64::from(PAGE_SIZE) > 1u64 << 32
            {
                return Err(FileError32::InvalidParameter);
            }
            previous_end = end;
            for page in range.indices() {
                memory
                    .permissions_at(GuestAddress(page as u32 * PAGE_SIZE))
                    .map_err(memory_error)?;
            }
        }
        let mut owned_words = Vec::new();
        owned_words
            .try_reserve_exact(words.len())
            .map_err(|_| FileError32::Allocation)?;
        owned_words.extend_from_slice(words);
        owned_words.sort_unstable_by_key(|word| word.address.0);
        let mut previous_end = 0u64;
        let mut range_index = 0;
        for word in &owned_words {
            let start = u64::from(word.address.0);
            if start < previous_end {
                return Err(FileError32::InvalidParameter);
            }
            previous_end = start + 4;
            memory
                .check_span(word.address, 4, Access::Write)
                .map_err(memory_error)?;
            while range_index < ranges.len()
                && (ranges[range_index].first + ranges[range_index].count) as u64
                    * u64::from(PAGE_SIZE)
                    <= start
            {
                range_index += 1;
            }
            if ranges
                .get(range_index)
                .is_some_and(|range| range.first as u64 * u64::from(PAGE_SIZE) < start + 4)
            {
                return Err(FileError32::InvalidParameter);
            }
        }
        if ranges.is_empty() && owned_words.len() > 131_072 {
            return Err(FileError32::InvalidParameter);
        }
        memory
            .ensure_transaction_versions(u64::from(!ranges.is_empty() || !owned_words.is_empty()))
            .map_err(memory_error)?;
        Ok(PreparedMappingExit32 {
            ranges,
            words: owned_words,
            version: memory.transaction_version(),
        })
    }
    pub(in crate::process::windows::files) fn commit_process_exit(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedMappingExit32,
    ) {
        assert_eq!(memory.transaction_version(), prepared.version);
        if !prepared.ranges.is_empty() {
            memory
                .unmap_ranges_with_word_writes(&prepared.ranges, &prepared.words)
                .expect("exit mappings and words were preflighted under unchanged ownership");
        } else if !prepared.words.is_empty() {
            memory
                .write_thread_words32(&prepared.words)
                .expect("exit words were preflighted under unchanged ownership");
        }
        self.views.clear();
        for pages in self.pages.drain(..) {
            memory
                .release_shared_pages(pages.backing)
                .expect("exit backing ownership remains valid");
        }
        self.objects.clear();
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::Permissions,
        process::windows::files::{ordinary::FileCallOutcome32, tests::support::runtime},
        windows::files::FileApi32 as Api,
    };
    fn returned(outcome: Result<FileCallOutcome32, FileError32>) -> u32 {
        let FileCallOutcome32::Return { value, .. } = outcome.unwrap() else {
            panic!("pagefile bytes are resident");
        };
        value
    }
    fn fixture() -> (FileRuntime32, AddressSpace, u32, u32, PageRange) {
        let (mut files, mut memory, _) = runtime();
        let section = returned(files.execute_mapping(
            &mut memory,
            &[],
            Api::CreateFileMappingW,
            &[u32::MAX, 0, 4, 0, 4096, 0],
        ));
        let first = returned(files.execute_mapping(
            &mut memory,
            &[],
            Api::MapViewOfFile,
            &[section, 2, 0, 0, 0],
        ));
        let second = returned(files.execute_mapping(
            &mut memory,
            &[],
            Api::MapViewOfFile,
            &[section, 2, 0, 0, 0],
        ));
        let extra = PageRange::new(GuestAddress(0x9000), 1).unwrap();
        memory.map_zeroed(extra, Permissions::READ_WRITE).unwrap();
        memory
            .write(GuestAddress(0x1200), &2u32.to_le_bytes())
            .unwrap();
        (files, memory, first, second, extra)
    }
    #[test]
    fn process_exit_file_views_extra_scratch_and_array_unlock_publish_one_memory_version() {
        let (mut files, mut memory, first, second, extra) = fixture();
        assert!(files.mappings.pages.iter().any(|pages| {
            memory.is_shared_alias(GuestAddress(first), &pages.backing, 0)
                && memory.is_shared_alias(GuestAddress(second), &pages.backing, 0)
        }));
        let word = WordWrite32 {
            address: GuestAddress(0x1200),
            value: 1,
        };
        let protected = PageRange::new(GuestAddress(0x1000), 1).unwrap();
        memory.protect(protected, Permissions::READ).unwrap();
        let version = memory.transaction_version();
        let mapped = memory.mapped_pages();
        assert!(
            files
                .mappings
                .prepare_process_exit(&memory, &[extra], &[word])
                .is_err()
        );
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(memory.mapped_pages(), mapped);
        assert_eq!(files.mappings.views.len(), 2);
        assert!(!files.mappings.pages.is_empty());
        let mut value = [0; 4];
        memory.read(word.address, &mut value).unwrap();
        assert_eq!(value, 2u32.to_le_bytes());
        memory.protect(protected, Permissions::READ_WRITE).unwrap();
        let version = memory.transaction_version();
        let receipt = files
            .mappings
            .prepare_process_exit(&memory, &[extra], &[word])
            .unwrap();
        assert_eq!(memory.transaction_version(), version);
        files.mappings.commit_process_exit(&mut memory, receipt);
        assert_eq!(memory.transaction_version(), version + 1);
        memory.read(word.address, &mut value).unwrap();
        assert_eq!(value, 1u32.to_le_bytes());
        for address in [first, second, 0x9000] {
            assert!(memory.permissions_at(GuestAddress(address)).is_err());
        }
        // the two views alias one physical page; mapped_pages counts physical storage.
        assert_eq!(memory.mapped_pages(), mapped - 2);
        assert!(files.mappings.views.is_empty());
        assert!(files.mappings.pages.is_empty());
        assert!(files.mappings.objects.is_empty());
    }
    #[test]
    fn process_exit_word_overlap_and_exhausted_versions_leave_every_file_view_owned() {
        let (files, mut memory, first, _, extra) = fixture();
        let overlap = WordWrite32 {
            address: GuestAddress(first),
            value: 0,
        };
        let version = memory.transaction_version();
        assert!(matches!(
            files
                .mappings
                .prepare_process_exit(&memory, &[extra], &[overlap]),
            Err(FileError32::InvalidParameter)
        ));
        assert_eq!(memory.transaction_version(), version);
        assert_eq!(files.mappings.views.len(), 2);
        memory.exhaust_versions_for_test();
        let mapped = memory.mapped_pages();
        assert!(
            files
                .mappings
                .prepare_process_exit(
                    &memory,
                    &[extra],
                    &[WordWrite32 {
                        address: GuestAddress(0x1200),
                        value: 1
                    }]
                )
                .is_err()
        );
        assert_eq!(memory.transaction_version(), u64::MAX);
        assert_eq!(memory.mapped_pages(), mapped);
        assert_eq!(files.mappings.views.len(), 2);
        let mut value = [0; 4];
        memory.read(GuestAddress(0x1200), &mut value).unwrap();
        assert_eq!(value, 2u32.to_le_bytes());
    }
}
