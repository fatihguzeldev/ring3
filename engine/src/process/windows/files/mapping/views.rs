use super::context::*;
use crate::{
    files::{FileError32, FileIo32},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions},
    process::windows::files::{FileRuntime32, ordinary::FileCallOutcome32},
};

pub(super) struct MapViewRequest32 {
    pub(super) object: u64,
    pub(super) access: u32,
    pub(super) offset: u64,
    pub(super) length: u32,
    pub(super) base: u32,
}

impl FileRuntime32 {
    pub(super) fn map_file_view(
        &mut self,
        memory: &mut AddressSpace,
        reserved: &[PageRange],
        request: MapViewRequest32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let MapViewRequest32 {
            object: id,
            access,
            offset,
            length,
            base,
        } = request;
        let object_index = self.mappings.index(id)?;
        let object = &self.mappings.objects[object_index];
        if !offset.is_multiple_of(65_536) || !base.is_multiple_of(65_536) {
            return Err(FileError32::MappedAlignment);
        }
        let (permissions, copy) = object.protection.view(access)?;
        let length = if length == 0 {
            object
                .size
                .checked_sub(offset)
                .ok_or(FileError32::InvalidParameter)?
        } else {
            u64::from(length)
        };
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > object.size)
            || length > u64::from(u32::MAX)
        {
            return Err(FileError32::InvalidParameter);
        }
        let length = length as usize;
        let count = length.div_ceil(PAGE_SIZE as usize);
        if self.mappings.views.len() >= self.mappings.limits.views {
            return Err(FileError32::Capacity);
        }
        if object.references == u32::MAX {
            return Err(FileError32::Exhausted);
        }
        let key = object.key;
        let range = self
            .mappings
            .choose_range(memory, reserved, count as u32, base)?;
        let mut selected = Vec::new();
        selected
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        let mut missing = Vec::new();
        missing
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for index in 0..count {
            let at = offset + index as u64 * u64::from(PAGE_SIZE);
            let cached = self
                .mappings
                .pages
                .iter()
                .enumerate()
                .find_map(|(group, pages)| {
                    (pages.key == key)
                        .then(|| {
                            pages
                                .offsets
                                .binary_search(&at)
                                .ok()
                                .map(|page| (group, page))
                        })
                        .flatten()
                });
            if let Some(position) = cached {
                selected.push(position)
            } else {
                selected.push((self.mappings.pages.len(), missing.len()));
                missing.push(at)
            }
        }
        memory
            .prepare_alias_capacity(missing.len(), if copy { count } else { 0 })
            .map_err(memory_error)?;
        let previous = self.mappings.pending.take();
        let mut stage = if let Some(stage) = previous {
            if (
                stage.object,
                stage.access,
                stage.offset,
                stage.length,
                stage.base,
            ) != (id, access, offset, length, base)
                || stage.missing != missing
            {
                return Err(FileError32::Busy);
            }
            stage
        } else {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(missing.len() * PAGE_SIZE as usize)
                .map_err(|_| FileError32::Allocation)?;
            bytes.resize(missing.len() * PAGE_SIZE as usize, 0);
            PendingView32 {
                object: id,
                access,
                offset,
                length,
                base,
                missing,
                bytes,
                filled: 0,
            }
        };
        while stage.filled < stage.missing.len() {
            let at = stage.missing[stage.filled];
            let start = stage.filled * PAGE_SIZE as usize;
            if let MappingBacking32::File(file) = key {
                let read_length = (self.vfs.info(file)?.size.saturating_sub(at))
                    .min(u64::from(PAGE_SIZE)) as usize;
                match self.vfs.prepare_read_at(file, at, read_length)? {
                    FileIo32::NeedBytes(request) => {
                        self.mappings.pending = Some(stage);
                        return Ok(FileCallOutcome32::PendingBytes(request));
                    }
                    FileIo32::Ready(read) => {
                        read.validate(&self.vfs)?;
                        stage.bytes[start..start + read.bytes().len()]
                            .copy_from_slice(read.bytes());
                    }
                }
            }
            stage.filled += 1;
        }
        let PendingView32 { missing, bytes, .. } = stage;
        let mut aliases = Vec::new();
        aliases
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        let mut committed = Vec::new();
        committed
            .try_reserve_exact(missing.len())
            .map_err(|_| FileError32::Allocation)?;
        committed.resize(missing.len(), true);
        let mut flushed = Vec::new();
        flushed
            .try_reserve_exact(missing.len())
            .map_err(|_| FileError32::Allocation)?;
        if !missing.is_empty() {
            self.mappings
                .pages
                .try_reserve(1)
                .map_err(|_| FileError32::Allocation)?;
        }
        memory
            .ensure_transaction_versions(1 + u64::from(!missing.is_empty()))
            .map_err(memory_error)?;
        if !missing.is_empty() {
            let prepared = memory
                .prepare_shared_pages(missing.len() as u32, &bytes)
                .map_err(memory_error)?;
            let backing = memory.commit_shared_pages(prepared);
            for index in 0..missing.len() {
                flushed.push(memory.shared_content_version(&backing, index).unwrap());
            }
            self.mappings.pages.push(MappingPage32 {
                key,
                offsets: missing,
                backing,
                flushed,
                committed,
            });
        }
        for &(group, page) in &selected {
            let rights = if self.mappings.objects[object_index].protection.reserve
                && !self.mappings.pages[group].committed[page]
            {
                Permissions::NONE
            } else {
                permissions
            };
            aliases.push((&self.mappings.pages[group].backing, page, rights));
        }
        memory
            .map_shared_page_rights(range, &aliases, copy)
            .expect("mapping capacity, guest extent, aliases and version were preflighted");
        let address = range.first as u32 * PAGE_SIZE;
        self.mappings.objects[object_index].references += 1;
        self.mappings.views.push(MappingView32 {
            object: id,
            address,
            length,
            offset,
            copy,
            range,
            permissions,
        });
        Ok(FileCallOutcome32::success(address))
    }
}
impl MappingRuntime32 {
    fn choose_range(
        &self,
        memory: &AddressSpace,
        reserved: &[PageRange],
        count: u32,
        base: u32,
    ) -> Result<PageRange, FileError32> {
        let mut bounds = self.bounds;
        if base != 0 {
            let range = PageRange::new(GuestAddress(base), count)
                .map_err(|_| FileError32::InvalidAddress)?;
            if overlaps_reserved(range, reserved)
                || self.views.iter().any(|view| overlap(range, view.range))
            {
                return Err(FileError32::InvalidAddress);
            }
            let found = memory
                .find_alias_range(range, count, 65_536)
                .map_err(|_| FileError32::InvalidAddress)?;
            return Ok(found);
        }
        loop {
            let range = memory
                .find_alias_range(bounds, count, 65_536)
                .map_err(memory_error)?;
            let end = reserved
                .iter()
                .chain(self.views.iter().map(|view| &view.range))
                .filter(|&&reserved| overlap(range, reserved))
                .map(|reserved| reserved.first + reserved.count)
                .max();
            let Some(end) = end else { return Ok(range) };
            let limit = bounds.first + bounds.count;
            if end >= limit {
                return Err(FileError32::Capacity);
            }
            bounds = PageRange {
                first: end,
                count: limit - end,
            };
        }
    }
}
fn overlaps_reserved(range: PageRange, reserved: &[PageRange]) -> bool {
    reserved.iter().any(|&reserved| overlap(range, reserved))
}
fn overlap(a: PageRange, b: PageRange) -> bool {
    a.first < b.first + b.count && b.first < a.first + a.count
}
pub(super) fn memory_error(error: crate::memory::MemoryError) -> FileError32 {
    match error {
        crate::memory::MemoryError::Allocation => FileError32::Allocation,
        crate::memory::MemoryError::VersionExhausted => FileError32::Exhausted,
        crate::memory::MemoryError::Capacity => FileError32::Capacity,
        _ => FileError32::InvalidAddress,
    }
}
