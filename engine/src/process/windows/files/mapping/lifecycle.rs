use super::context::*;
use super::views::memory_error;
use crate::{
    files::{FileError32, FileId, FileIo32},
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
            if let Some(open) = self.mappings.objects[index].source {
                let key = self.mappings.objects[index].key;
                self.vfs.check_close_open_after_mutations(
                    open,
                    u64::from(self.mapping_backing_dirty(memory, key)?),
                )?;
            }
            let key = self.mappings.objects[index].key;
            if self
                .mappings
                .objects
                .iter()
                .filter(|object| object.key == key)
                .count()
                == 1
            {
                if let Some(request) = self.flush_mapping_pages(memory, key, None)? {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
                self.drop_mapping_pages(memory, key)?;
            }
            let object = self.mappings.objects.remove(index);
            if let Some(open) = object.source {
                self.vfs.close_open(open)?;
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
        if final_object && let Some(open) = self.mappings.objects[object_index].source {
            self.vfs.check_close_open_after_mutations(
                open,
                u64::from(final_backing && self.mapping_backing_dirty(memory, key)?),
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
        if final_backing && let Some(request) = self.flush_mapping_pages(memory, key, None)? {
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
    fn mapping_backing_dirty(
        &self,
        memory: &AddressSpace,
        key: MappingBacking32,
    ) -> Result<bool, FileError32> {
        if !matches!(key, MappingBacking32::File(_)) {
            return Ok(false);
        }
        for pages in self.mappings.pages.iter().filter(|pages| pages.key == key) {
            for (index, &flushed) in pages.flushed.iter().enumerate() {
                if memory
                    .shared_content_version(&pages.backing, index)
                    .map_err(memory_error)?
                    != flushed
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    fn flush_mapping_pages(
        &mut self,
        memory: &AddressSpace,
        key: MappingBacking32,
        range: Option<(u64, u64)>,
    ) -> Result<Option<crate::files::FileReadRequest32>, FileError32> {
        let MappingBacking32::File(file) = key else {
            return Ok(None);
        };
        let file_size = self.vfs.info(file)?.size;
        let mut selected = Vec::new();
        let mut data = Vec::new();
        for (group, pages) in self
            .mappings
            .pages
            .iter()
            .enumerate()
            .filter(|(_, pages)| pages.key == key)
        {
            for (page, &offset) in pages.offsets.iter().enumerate() {
                if range.is_some_and(|(start, end)| {
                    offset + u64::from(PAGE_SIZE) <= start || offset >= end
                }) {
                    continue;
                }
                let version = memory
                    .shared_content_version(&pages.backing, page)
                    .map_err(memory_error)?;
                if version == pages.flushed[page] || offset >= file_size {
                    continue;
                }
                let length = (file_size - offset).min(u64::from(PAGE_SIZE)) as usize;
                selected
                    .try_reserve(1)
                    .map_err(|_| FileError32::Allocation)?;
                data.try_reserve(length)
                    .map_err(|_| FileError32::Allocation)?;
                let at = data.len();
                data.resize(at + length, 0);
                memory
                    .read_shared_pages(
                        &pages.backing,
                        page * PAGE_SIZE as usize,
                        &mut data[at..at + length],
                    )
                    .map_err(memory_error)?;
                selected.push((group, page, offset, at, length, version));
            }
        }
        if selected.is_empty() {
            return Ok(None);
        }
        let mut spans = Vec::new();
        spans
            .try_reserve_exact(selected.len())
            .map_err(|_| FileError32::Allocation)?;
        for &(_, _, offset, at, length, _) in &selected {
            spans.push((offset, &data[at..at + length]));
        }
        match self.vfs.prepare_writes_at(file, &spans)? {
            FileIo32::NeedBytes(request) => Ok(Some(request)),
            FileIo32::Ready(write) => {
                write.validate(&self.vfs)?;
                self.vfs.commit_write(write);
                for (group, page, _, _, _, version) in selected {
                    self.mappings.pages[group].flushed[page] = version;
                }
                Ok(None)
            }
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
            if permissions.allows(access) && !view.permissions.allows(access) {
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
            if permissions.allows(access) && !view.permissions.allows(access) {
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
