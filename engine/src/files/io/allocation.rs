use super::*;

pub(crate) struct PreparedAllocation32 {
    epoch: u64,
    revision: u64,
    next: u64,
    file: FileId,
    open: OpenFileId,
    size: u64,
    allocation: u64,
    content_revision: u64,
    pages: Vec<OwnedPage32>,
}
impl PreparedAllocation32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}

fn extent(size: u64) -> Result<u64, FileError32> {
    size.checked_add(FILE_PAGE_BYTES as u64 - 1)
        .map(|size| size & !(FILE_PAGE_BYTES as u64 - 1))
        .ok_or(FileError32::InvalidParameter)
}

impl Vfs32 {
    pub(crate) fn allocation_size(&self, file: FileId) -> Result<u64, FileError32> {
        let node = self.node(file)?;
        if node.directory() {
            return Ok(0);
        }
        Ok(extent(node.size)?.max(node.content.allocation))
    }
    pub(crate) fn prepare_allocation(
        &mut self,
        open: OpenFileId,
        requested: u64,
    ) -> Result<PreparedAllocation32, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::WRITE) {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        let node = self.node(file)?;
        if node.directory() {
            return Err(FileError32::AccessDenied);
        }
        if self.has_read_only_section(file) {
            return Err(FileError32::SharingViolation);
        }
        let allocation = extent(requested)?;
        // immutable mounted content already owns external storage; sparse zero holes do not.
        let backed_extent = if node.content.asset.is_some() {
            extent(node.content.visible)?
        } else {
            0
        };
        let size = node.size.min(requested);
        let content_revision = if size == node.size {
            node.content_revision
        } else {
            node.content_revision
                .checked_add(1)
                .ok_or(FileError32::Exhausted)?
        };
        let (revision, next) = self.stamp()?;
        let retained = node
            .content
            .pages
            .iter()
            .filter(|page| page.offset < allocation)
            .count();
        let tail_pages =
            usize::try_from(allocation.saturating_sub(backed_extent) / FILE_PAGE_BYTES as u64)
                .map_err(|_| FileError32::DiskFull)?;
        let present_tail = node
            .content
            .pages
            .iter()
            .filter(|page| page.offset >= backed_extent && page.offset < allocation)
            .count();
        let additional = tail_pages
            .checked_sub(present_tail)
            .ok_or(FileError32::InvalidParameter)?;
        let used = self.overlay_pages - (node.content.pages.len() - retained);
        if used.checked_add(additional).ok_or(FileError32::DiskFull)? > self.limits.overlay_pages {
            return Err(FileError32::DiskFull);
        }
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(additional)
            .map_err(|_| FileError32::Allocation)?;
        let mut offset = backed_extent;
        while offset < allocation {
            if node
                .content
                .pages
                .binary_search_by_key(&offset, |page| page.offset)
                .is_err()
            {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(FILE_PAGE_BYTES)
                    .map_err(|_| FileError32::Allocation)?;
                bytes.resize(FILE_PAGE_BYTES, 0);
                pages.push(OwnedPage32 {
                    offset,
                    bytes: bytes.into_boxed_slice(),
                });
            }
            offset += FILE_PAGE_BYTES as u64;
        }
        self.node_mut(file)?
            .content
            .pages
            .try_reserve(additional)
            .map_err(|_| FileError32::Allocation)?;
        Ok(PreparedAllocation32 {
            epoch: self.epoch,
            revision,
            next,
            file,
            open,
            size,
            allocation,
            content_revision,
            pages,
        })
    }
    pub(crate) fn commit_allocation(&mut self, prepared: PreparedAllocation32) {
        prepared
            .validate(self)
            .expect("prepared file allocation authority");
        let description = self.open(prepared.open).unwrap();
        let suppress_write = description.suppress_write;
        let suppress_change = description.suppress_change;
        let now = self.now;
        let node = self.node_mut(prepared.file).unwrap();
        let old_pages = node.content.pages.len();
        let old_size = node.size;
        node.content
            .pages
            .retain(|page| page.offset < prepared.allocation);
        node.content.pages.extend(prepared.pages);
        node.content.pages.sort_unstable_by_key(|page| page.offset);
        if prepared.size < old_size {
            for page in &mut node.content.pages {
                let start = prepared
                    .size
                    .saturating_sub(page.offset)
                    .min(FILE_PAGE_BYTES as u64) as usize;
                page.bytes[start..].fill(0);
            }
            node.content.visible = node.content.visible.min(prepared.size);
            if !suppress_write {
                node.times.written = now;
            }
        }
        node.content.allocation = prepared.allocation;
        node.content_revision = prepared.content_revision;
        node.size = prepared.size;
        node.dirty = true;
        if !suppress_change {
            node.changed = now;
        }
        let pages = node.content.pages.len();
        self.overlay_pages = self.overlay_pages - old_pages + pages;
        if prepared.size != old_size {
            self.cache.invalidate(prepared.file);
            self.pending_writes
                .retain(|stage| stage.file != prepared.file);
        }
        self.revision = prepared.next;
    }
}
