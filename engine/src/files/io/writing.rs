use super::*;

impl Vfs32 {
    pub(crate) fn prepare_write(
        &mut self,
        open: OpenFileId,
        bytes: &[u8],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        let description = self.open(open)?;
        if description.options.access.0 & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0) == 0 {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        let position = if description.options.access.contains(FileAccess32::WRITE) {
            description.position
        } else {
            self.node(file)?.size
        };
        description
            .cursor_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        match self.prepare_write_at(file, position, bytes)? {
            FileIo32::NeedBytes(request) => Ok(FileIo32::NeedBytes(request)),
            FileIo32::Ready(mut prepared) => {
                prepared.cursor = Some((
                    open,
                    position
                        .checked_add(bytes.len() as u64)
                        .ok_or(FileError32::InvalidParameter)?,
                ));
                Ok(FileIo32::Ready(prepared))
            }
        }
    }
    pub(crate) fn prepare_write_positioned(
        &mut self,
        open: OpenFileId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        let description = self.open(open)?;
        if description.options.access.0 & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0) == 0 {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        let offset = if description.options.access.contains(FileAccess32::WRITE) {
            offset
        } else {
            self.node(file)?.size
        };
        description
            .cursor_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        match self.prepare_write_at(file, offset, bytes)? {
            FileIo32::NeedBytes(request) => Ok(FileIo32::NeedBytes(request)),
            FileIo32::Ready(mut receipt) => {
                receipt.cursor = Some((
                    open,
                    offset
                        .checked_add(bytes.len() as u64)
                        .ok_or(FileError32::InvalidParameter)?,
                ));
                Ok(FileIo32::Ready(receipt))
            }
        }
    }
    pub(crate) fn prepare_write_at(
        &mut self,
        file: FileId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        self.prepare_writes_at(file, &[(offset, bytes)])
    }
    pub(crate) fn prepare_write_at_for(
        &mut self,
        operation: u64,
        file: FileId,
        offset: u64,
        bytes: &[u8],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        if operation == 0 || operation == u64::MAX {
            return Err(FileError32::InvalidParameter);
        }
        self.writes_at_owned(
            FileIoOwner32::AsyncOperation(operation),
            file,
            &[(offset, bytes)],
        )
    }
    pub(crate) fn prepare_writes_at(
        &mut self,
        file: FileId,
        spans: &[(u64, &[u8])],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        self.writes_at_owned(FileIoOwner32::GuestThread(self.io_thread), file, spans)
    }
    fn writes_at_owned(
        &mut self,
        owner: FileIoOwner32,
        file: FileId,
        spans: &[(u64, &[u8])],
    ) -> Result<FileIo32<PreparedWrite32>, FileError32> {
        let node = self.node(file)?;
        if node.directory() {
            return Err(FileError32::AccessDenied);
        }
        let content_revision = node.content_revision;
        let old_size = node.size;
        let next_content = content_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let count = spans
            .iter()
            .try_fold(0usize, |n, (_, b)| n.checked_add(b.len()))
            .ok_or(FileError32::Capacity)?;
        if count > self.limits.max_io_bytes {
            return Err(FileError32::Capacity);
        }
        let (revision, next) = self.stamp()?;
        let index = self.pending_writes.iter().position(|s| {
            s.owner == owner
                && s.file == file
                && s.revision == content_revision
                && s.spans.len() == spans.len()
                && s.spans
                    .iter()
                    .zip(spans)
                    .all(|((a, b), (x, y))| a == x && b.as_slice() == *y)
        });
        let mut stage = if let Some(index) = index {
            self.pending_writes.swap_remove(index)
        } else {
            let mut offsets = Vec::new();
            let mut owned_spans = Vec::new();
            owned_spans
                .try_reserve_exact(spans.len())
                .map_err(|_| FileError32::Allocation)?;
            for &(offset, bytes) in spans {
                let end = offset
                    .checked_add(bytes.len() as u64)
                    .ok_or(FileError32::InvalidParameter)?;
                if !bytes.is_empty() {
                    let mut page = offset / FILE_PAGE_BYTES as u64 * FILE_PAGE_BYTES as u64;
                    while page < end {
                        offsets
                            .try_reserve(1)
                            .map_err(|_| FileError32::Allocation)?;
                        offsets.push(page);
                        page = page
                            .checked_add(FILE_PAGE_BYTES as u64)
                            .ok_or(FileError32::InvalidParameter)?;
                    }
                }
                let mut copy = Vec::new();
                copy.try_reserve_exact(bytes.len())
                    .map_err(|_| FileError32::Allocation)?;
                copy.extend_from_slice(bytes);
                owned_spans.push((offset, copy));
            }
            offsets.sort_unstable();
            offsets.dedup();
            let additional = offsets
                .iter()
                .filter(|o| {
                    !self
                        .node(file)
                        .unwrap()
                        .content
                        .pages
                        .iter()
                        .any(|p| p.offset == **o)
                })
                .count();
            if self
                .overlay_pages
                .checked_add(additional)
                .ok_or(FileError32::DiskFull)?
                > self.limits.overlay_pages
            {
                return Err(FileError32::DiskFull);
            }
            let planned = offsets
                .len()
                .checked_mul(FILE_PAGE_BYTES)
                .and_then(|n| n.checked_add(count))
                .ok_or(FileError32::Capacity)?;
            let used = self
                .pending_writes
                .iter()
                .map(|s| {
                    s.offsets.len() * FILE_PAGE_BYTES
                        + s.spans.iter().map(|(_, b)| b.len()).sum::<usize>()
                })
                .sum::<usize>();
            if self.pending_writes.len() >= self.limits.requests
                || used.checked_add(planned).ok_or(FileError32::Capacity)?
                    > self
                        .limits
                        .max_io_bytes
                        .saturating_mul(2)
                        .saturating_add(2 * FILE_PAGE_BYTES)
            {
                return Err(FileError32::Capacity);
            }
            let mut pages = Vec::new();
            pages
                .try_reserve_exact(offsets.len())
                .map_err(|_| FileError32::Allocation)?;
            WriteStage32 {
                owner,
                file,
                revision: content_revision,
                spans: owned_spans,
                offsets,
                pages,
                waiting: None,
            }
        };
        while stage.pages.len() < stage.offsets.len() {
            let page = stage.offsets[stage.pages.len()];
            let page_end = page
                .checked_add(FILE_PAGE_BYTES as u64)
                .ok_or(FileError32::InvalidParameter)?;
            let full = stage.spans.iter().any(|(offset, bytes)| {
                *offset <= page
                    && offset
                        .checked_add(bytes.len() as u64)
                        .is_some_and(|end| end >= page_end)
            });
            let mut bytes = if full {
                let mut data = Vec::new();
                data.try_reserve_exact(FILE_PAGE_BYTES)
                    .map_err(|_| FileError32::Allocation)?;
                data.resize(FILE_PAGE_BYTES, 0);
                data
            } else {
                match self.read_at_owned(owner, file, page, FILE_PAGE_BYTES)? {
                    FileIo32::NeedBytes(request) => {
                        stage.waiting = Some(request);
                        self.pending_writes.push(stage);
                        return Ok(FileIo32::NeedBytes(request));
                    }
                    FileIo32::Ready(read) => {
                        let mut data = read.bytes;
                        data.try_reserve_exact(FILE_PAGE_BYTES - data.len())
                            .map_err(|_| FileError32::Allocation)?;
                        data.resize(FILE_PAGE_BYTES, 0);
                        data
                    }
                }
            };
            for (offset, input) in &stage.spans {
                let start = page.max(*offset);
                let end = page_end.min(*offset + input.len() as u64);
                if start < end {
                    bytes[(start - page) as usize..(end - page) as usize].copy_from_slice(
                        &input[(start - *offset) as usize..(end - *offset) as usize],
                    );
                }
            }
            stage.pages.push(OwnedPage32 {
                offset: page,
                bytes: bytes.into_boxed_slice(),
            });
            stage.waiting = None;
        }
        let size = stage
            .spans
            .iter()
            .filter(|(_, bytes)| !bytes.is_empty())
            .map(|(o, b)| o + b.len() as u64)
            .max()
            .unwrap_or(old_size)
            .max(old_size);
        let additional = stage
            .pages
            .iter()
            .filter(|p| {
                !self
                    .node(file)
                    .unwrap()
                    .content
                    .pages
                    .iter()
                    .any(|q| q.offset == p.offset)
            })
            .count();
        if self
            .overlay_pages
            .checked_add(additional)
            .ok_or(FileError32::DiskFull)?
            > self.limits.overlay_pages
        {
            return Err(FileError32::DiskFull);
        }
        self.node_mut(file)?
            .content
            .pages
            .try_reserve(additional)
            .map_err(|_| FileError32::Allocation)?;
        Ok(FileIo32::Ready(PreparedWrite32 {
            epoch: self.epoch,
            revision,
            next,
            file,
            pages: stage.pages,
            size,
            count,
            content_revision: next_content,
            cursor: None,
        }))
    }
    pub(crate) fn commit_write(&mut self, prepared: PreparedWrite32) -> usize {
        prepared
            .validate(self)
            .expect("prepared file write authority");
        if prepared.count == 0 {
            if let Some((open, position)) = prepared.cursor {
                let description = self.open_mut(open).unwrap();
                description.position = position;
                description.cursor_revision += 1;
                self.revision = prepared.next;
            }
            return 0;
        }
        let now = self.now;
        let suppress = prepared
            .cursor
            .and_then(|(o, _)| self.open(o).ok().map(|o| o.suppress_write))
            .unwrap_or(false);
        let mut additional = 0;
        {
            let node = self.node_mut(prepared.file).unwrap();
            for page in prepared.pages {
                match node
                    .content
                    .pages
                    .binary_search_by_key(&page.offset, |p| p.offset)
                {
                    Ok(index) => node.content.pages[index] = page,
                    Err(index) => {
                        node.content.pages.insert(index, page);
                        additional += 1;
                    }
                }
            }
            node.size = prepared.size;
            node.content_revision = prepared.content_revision;
            node.dirty = true;
            node.changed = now;
            node.attributes = (node.attributes & !ATTR_NORMAL) | ATTR_ARCHIVE;
            if !suppress {
                node.times.written = now;
            }
        }
        self.overlay_pages += additional;
        self.cache.invalidate(prepared.file);
        self.pending_writes.retain(|s| s.file != prepared.file);
        if let Some((open, position)) = prepared.cursor {
            let description = self.open_mut(open).unwrap();
            description.position = position;
            description.cursor_revision += 1;
        }
        self.revision = prepared.next;
        prepared.count
    }
}
