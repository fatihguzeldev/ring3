use super::context::*;
use super::views::memory_error;
use crate::{
    files::{FileError32, FileId},
    memory::{Access, AddressSpace, PAGE_SIZE, PageRange, Permissions},
    process::windows::files::{FileRuntime32, ordinary::FileCallOutcome32},
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn close_mapping(
        &mut self,
        memory: &mut AddressSpace,
        id: u64,
    ) -> Result<FileCallOutcome32, FileError32> {
        let index = self.mappings.index(id)?;
        if self.mappings.objects[index].handles == 0 {
            return Err(FileError32::InvalidHandle);
        }
        let result = self.release_mapping_reference(memory, id)?;
        if matches!(result, FileCallOutcome32::Return { value: 1, .. })
            && let Ok(index) = self.mappings.index(id)
        {
            self.mappings.objects[index].handles -= 1;
        }
        Ok(result)
    }
    fn release_mapping_reference(
        &mut self,
        memory: &mut AddressSpace,
        id: u64,
    ) -> Result<FileCallOutcome32, FileError32> {
        let index = self.mappings.index(id)?;
        if self.mappings.objects[index].references == 1 {
            let key = self.mappings.objects[index].key;
            let final_backing = self
                .mappings
                .objects
                .iter()
                .filter(|object| object.key == key)
                .count()
                == 1;
            let writeback = if final_backing {
                Some(self.prepare_mapping_writeback(memory, key, None)?)
            } else {
                None
            };
            if let Some(open) = self.mappings.objects[index].source {
                self.vfs.check_close_open_after_mutations(
                    open,
                    writeback.as_ref().map_or(0, |plan| plan.mutations()),
                )?;
            }
            if let Some(writeback) = writeback {
                if let Some(request) = self.flush_mapping_writeback(memory, writeback)? {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
                self.drop_mapping_pages(memory, key)?;
            }
            let object = self.mappings.objects.remove(index);
            if let Some(open) = object.source {
                self.vfs.close_open(open)?;
                self.retire_completion_file(open);
            }
        } else {
            self.mappings.objects[index].references -= 1;
        }
        Ok(FileCallOutcome32::success(1))
    }
    pub(in crate::process::windows::files) fn unmap_file_view(
        &mut self,
        memory: &mut AddressSpace,
        address: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let view_index = self
            .mappings
            .views
            .iter()
            .position(|view| view.address == address)
            .ok_or(FileError32::InvalidAddress)?;
        let view = &self.mappings.views[view_index];
        let view_range = view.range;
        let object_index = self.mappings.index(view.object)?;
        let key = self.mappings.objects[object_index].key;
        let final_object = self.mappings.objects[object_index].references == 1;
        let final_backing = final_object
            && self
                .mappings
                .objects
                .iter()
                .filter(|object| object.key == key)
                .count()
                == 1;
        let writeback = if final_backing {
            Some(self.prepare_mapping_writeback(memory, key, None)?)
        } else {
            None
        };
        if final_object && let Some(open) = self.mappings.objects[object_index].source {
            self.vfs.check_close_open_after_mutations(
                open,
                writeback.as_ref().map_or(0, |plan| plan.mutations()),
            )?;
        }
        for page in view_range.indices() {
            memory
                .permissions_at(crate::memory::GuestAddress(page as u32 * PAGE_SIZE))
                .map_err(memory_error)?;
        }
        memory
            .ensure_transaction_versions(1)
            .map_err(memory_error)?;
        if let Some(writeback) = writeback
            && let Some(request) = self.flush_mapping_writeback(memory, writeback)?
        {
            return Ok(FileCallOutcome32::PendingBytes(request));
        }
        for page in view_range.indices() {
            memory
                .permissions_at(crate::memory::GuestAddress(page as u32 * PAGE_SIZE))
                .map_err(memory_error)?;
        }
        let view = self.mappings.views.remove(view_index);
        memory.unmap(view.range).map_err(memory_error)?;
        self.release_mapping_reference(memory, view.object)
    }
    pub(in crate::process::windows::files) fn flush_file_view(
        &mut self,
        memory: &AddressSpace,
        address: u32,
        length: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let view = self
            .mappings
            .views
            .iter()
            .find(|view| {
                u64::from(address) >= u64::from(view.address)
                    && u64::from(address) < u64::from(view.address) + view.length as u64
            })
            .ok_or(FileError32::InvalidAddress)?;
        let within = u64::from(address - view.address);
        let length = if length == 0 {
            view.length as u64 - within
        } else {
            u64::from(length)
        };
        if within + length > view.length as u64 {
            return Err(FileError32::InvalidParameter);
        }
        if view.copy {
            return Ok(FileCallOutcome32::success(1));
        }
        let key = self.mappings.objects[self.mappings.index(view.object)?].key;
        let start = view.offset + within;
        let end = start + length;
        if let Some(request) = self.flush_mapping_pages(memory, key, Some((start, end)))? {
            Ok(FileCallOutcome32::PendingBytes(request))
        } else {
            Ok(FileCallOutcome32::success(1))
        }
    }
    fn drop_mapping_pages(
        &mut self,
        memory: &mut AddressSpace,
        key: MappingBacking32,
    ) -> Result<(), FileError32> {
        let mut index = 0;
        while index < self.mappings.pages.len() {
            if self.mappings.pages[index].key == key {
                let pages = self.mappings.pages.remove(index);
                memory
                    .release_shared_pages(pages.backing)
                    .map_err(memory_error)?;
            } else {
                index += 1;
            }
        }
        Ok(())
    }
    pub(in crate::process::windows::files) fn try_commit_reserved_view(
        &mut self,
        memory: &mut AddressSpace,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<bool, FileError32> {
        let Some(view) = self.mappings.views.iter().find(|view| {
            range.first >= view.range.first
                && range.first + range.count <= view.range.first + view.range.count
        }) else {
            return Ok(false);
        };
        let object = &self.mappings.objects[self.mappings.index(view.object)?];
        if !object.protection.reserve {
            return Ok(false);
        }
        for access in [Access::Read, Access::Write, Access::Execute] {
            if permissions.without_guard().allows(access) && !view.permissions.allows(access) {
                return Err(FileError32::AccessDenied);
            }
        }
        let key = object.key;
        let current = view.address;
        let start = view.offset + (range.first - view.range.first) as u64 * u64::from(PAGE_SIZE);
        let end = start + range.count as u64 * u64::from(PAGE_SIZE);
        let mut changes = Vec::new();
        for other in &self.mappings.views {
            if self.mappings.objects[self.mappings.index(other.object)?].key != key {
                continue;
            }
            let first = start.max(other.offset);
            let last = end.min(other.offset + other.range.count as u64 * u64::from(PAGE_SIZE));
            let mut offset = first;
            while offset < last {
                let committed = self
                    .mappings
                    .pages
                    .iter()
                    .filter(|pages| pages.key == key)
                    .find_map(|pages| {
                        pages
                            .offsets
                            .binary_search(&offset)
                            .ok()
                            .map(|page| pages.committed[page])
                    })
                    .unwrap();
                if other.address == current || !committed {
                    changes
                        .try_reserve(1)
                        .map_err(|_| FileError32::Allocation)?;
                    changes.push((
                        crate::memory::GuestAddress(other.address + (offset - other.offset) as u32),
                        if other.address == current {
                            permissions
                        } else {
                            other.permissions
                        },
                    ));
                }
                offset += u64::from(PAGE_SIZE);
            }
        }
        memory.protect_alias_pages(&changes).map_err(memory_error)?;
        for pages in self
            .mappings
            .pages
            .iter_mut()
            .filter(|pages| pages.key == key)
        {
            for (index, &offset) in pages.offsets.iter().enumerate() {
                if offset >= start && offset < end {
                    pages.committed[index] = true;
                }
            }
        }

        Ok(true)
    }
}
impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn minimum_file_size(&self, file: FileId) -> u64 {
        self.objects
            .iter()
            .filter(|object| object.key == MappingBacking32::File(file))
            .map(|object| object.size)
            .max()
            .unwrap_or(0)
    }
    pub(in crate::process::windows::files) fn contains_range(&self, range: PageRange) -> bool {
        self.views.iter().any(|view| {
            range.first < view.range.first + view.range.count
                && view.range.first < range.first + range.count
        })
    }
}

impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn validate_view_protection(
        &self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<bool, FileError32> {
        let Some(view) = self.views.iter().find(|view| {
            range.first < view.range.first + view.range.count
                && view.range.first < range.first + range.count
        }) else {
            return Ok(false);
        };
        if range.first < view.range.first
            || range.first + range.count > view.range.first + view.range.count
        {
            return Err(FileError32::InvalidAddress);
        }
        for access in [Access::Read, Access::Write, Access::Execute] {
            if permissions.without_guard().allows(access) && !view.permissions.allows(access) {
                return Err(FileError32::AccessDenied);
            }
        }
        let object = &self.objects[self.index(view.object)?];
        if object.protection.reserve {
            let start =
                view.offset + (range.first - view.range.first) as u64 * u64::from(PAGE_SIZE);
            let end = start + range.count as u64 * u64::from(PAGE_SIZE);
            if self
                .pages
                .iter()
                .filter(|pages| pages.key == object.key)
                .any(|pages| {
                    pages.offsets.iter().enumerate().any(|(index, &offset)| {
                        offset >= start && offset < end && !pages.committed[index]
                    })
                })
            {
                return Err(FileError32::InvalidAddress);
            }
        }
        Ok(true)
    }
}
