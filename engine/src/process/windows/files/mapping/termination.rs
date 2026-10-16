use super::{context::*, views::memory_error};
use crate::{
    files::{FileError32, FileReadRequest32},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE},
    process::windows::files::FileRuntime32,
};

pub(in crate::process::windows::files) struct PreparedMappingExit32 {
    version: u64,
    extra_unmap: Vec<crate::memory::PageRange>,
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
        extra_unmap: &[crate::memory::PageRange],
    ) -> Result<PreparedMappingExit32, FileError32> {
        memory
            .ensure_transaction_versions(
                self.views.len() as u64 + u64::from(!extra_unmap.is_empty()),
            )
            .map_err(memory_error)?;
        for view in &self.views {
            for page in view.range.indices() {
                memory
                    .permissions_at(GuestAddress(page as u32 * PAGE_SIZE))
                    .map_err(memory_error)?;
            }
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(extra_unmap.len())
            .map_err(|_| FileError32::Allocation)?;
        let mut end = 0;
        for range in extra_unmap {
            if range.first < end || range.count == 0 {
                return Err(FileError32::InvalidParameter);
            }
            end = range
                .first
                .checked_add(range.count)
                .ok_or(FileError32::InvalidParameter)?;
            for page in range.indices() {
                memory
                    .permissions_at(GuestAddress(page as u32 * PAGE_SIZE))
                    .map_err(memory_error)?;
            }
            if self.views.iter().any(|view| {
                view.range.first < end && range.first < view.range.first + view.range.count
            }) {
                return Err(FileError32::InvalidParameter);
            }
            owned.push(*range);
        }
        Ok(PreparedMappingExit32 {
            extra_unmap: owned,
            version: memory.transaction_version(),
        })
    }
    pub(in crate::process::windows::files) fn commit_process_exit(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedMappingExit32,
    ) {
        assert_eq!(memory.transaction_version(), prepared.version);
        for view in self.views.drain(..) {
            memory
                .unmap(view.range)
                .expect("exit view mappings and versions were preflighted");
        }
        if !prepared.extra_unmap.is_empty() {
            memory
                .unmap_ranges_with_word_writes(&prepared.extra_unmap, &[])
                .expect("provider exit mappings and versions were preflighted");
        }
        for pages in self.pages.drain(..) {
            memory
                .release_shared_pages(pages.backing)
                .expect("exit backing ownership remains valid");
        }
        self.objects.clear();
        self.pending = None;
    }
}
