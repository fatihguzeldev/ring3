use super::*;
use crate::files::content::ReadStage32;

impl Vfs32 {
    pub(crate) fn prepare_read(
        &mut self,
        open: OpenFileId,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::READ) {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        let position = description.position;
        description
            .cursor_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        match self.prepare_read_at(file, position, length)? {
            FileIo32::NeedBytes(request) => Ok(FileIo32::NeedBytes(request)),
            FileIo32::Ready(mut prepared) => {
                prepared.cursor = Some((
                    open,
                    position
                        .checked_add(prepared.bytes.len() as u64)
                        .ok_or(FileError32::InvalidParameter)?,
                ));
                prepared.next = self.stamp()?.1;
                Ok(FileIo32::Ready(prepared))
            }
        }
    }
    pub(crate) fn prepare_read_at(
        &mut self,
        file: FileId,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        self.read_at_owned(0, file, offset, length)
    }
    pub(crate) fn prepare_read_at_for(
        &mut self,
        operation: u64,
        file: FileId,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        if operation == 0 || operation == u64::MAX {
            return Err(FileError32::InvalidParameter);
        }
        self.read_at_owned(operation, file, offset, length)
    }
    pub(in crate::files) fn read_at_owned(
        &mut self,
        owner: u64,
        file: FileId,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        let node = self.node(file)?;
        if node.directory() {
            return Err(FileError32::AccessDenied);
        }
        let length = (node.size.saturating_sub(offset)).min(length as u64) as usize;
        let content_revision = node.content_revision;
        if length > self.limits.max_io_bytes {
            return Err(FileError32::Capacity);
        }
        let index = self.cache.stages.iter().position(|s| {
            s.owner == owner
                && s.file == file
                && s.revision == content_revision
                && s.offset == offset
                && s.length == length
        });
        let mut stage = if let Some(index) = index {
            self.cache.stages.swap_remove(index)
        } else {
            if self.cache.stages.len() >= self.limits.requests
                || self
                    .cache
                    .stages
                    .iter()
                    .map(|s| s.bytes.len())
                    .sum::<usize>()
                    .checked_add(length)
                    .ok_or(FileError32::Capacity)?
                    > self.limits.max_io_bytes
            {
                return Err(FileError32::Capacity);
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| FileError32::Allocation)?;
            bytes.resize(length, 0);
            ReadStage32 {
                owner,
                file,
                revision: content_revision,
                offset,
                length,
                bytes,
                filled: 0,
                waiting: None,
            }
        };
        while stage.filled < stage.length {
            let position = stage.offset + stage.filled as u64;
            let page = position / FILE_PAGE_BYTES as u64 * FILE_PAGE_BYTES as u64;
            let within = (position - page) as usize;
            let mut count = (FILE_PAGE_BYTES - within).min(stage.length - stage.filled);
            let node = self.node(file)?;
            if let Some(owned) = node.content.pages.iter().find(|p| p.offset == page) {
                stage.bytes[stage.filled..stage.filled + count]
                    .copy_from_slice(&owned.bytes[within..within + count]);
            } else if position < node.content.visible {
                count = (count as u64).min(node.content.visible - position) as usize;
                let asset = node.content.asset.ok_or(FileError32::InvalidParameter)?;
                if let Some(bytes) = self.cache.bytes(asset.id, page) {
                    stage.bytes[stage.filled..stage.filled + count]
                        .copy_from_slice(&bytes[within..within + count]);
                } else {
                    let request = self.cache.request(
                        FileReadRequest32 {
                            epoch: self.epoch,
                            request: 0,
                            file,
                            revision: content_revision,
                            asset: asset.id,
                            offset: page,
                            length: asset
                                .length
                                .saturating_sub(page)
                                .min(FILE_PAGE_BYTES as u64)
                                as u32,
                        },
                        stage.waiting,
                        self.limits.cache_pages,
                        self.limits.requests,
                    )?;
                    stage.waiting = Some(request);
                    self.cache.stages.push(stage);
                    return Ok(FileIo32::NeedBytes(request));
                }
            }
            stage.filled += count;
            if let Some(previous) = stage.waiting.take() {
                self.cache
                    .pages
                    .retain(|p| p.request != previous || p.supplied == p.request.length as usize);
            }
        }
        Ok(FileIo32::Ready(PreparedRead32 {
            epoch: self.epoch,
            revision: self.revision,
            next: self.revision,
            bytes: stage.bytes,
            cursor: None,
        }))
    }
    pub(crate) fn commit_read(&mut self, prepared: PreparedRead32) -> usize {
        prepared
            .validate(self)
            .expect("prepared file read authority");
        let count = prepared.bytes.len();
        if let Some((open, position)) = prepared.cursor {
            let now = self.now;
            let description = self.open_mut(open).unwrap();
            description.position = position;
            description.cursor_revision += 1;
            let file = description.file;
            let suppress = description.suppress_access;
            if !suppress && count > 0 {
                self.node_mut(file).unwrap().times.accessed = now;
            }
            self.revision = prepared.next;
        }
        count
    }
    pub(crate) fn prepare_read_positioned(
        &mut self,
        open: OpenFileId,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::READ) {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        description
            .cursor_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        match self.prepare_read_at(file, offset, length)? {
            FileIo32::NeedBytes(request) => Ok(FileIo32::NeedBytes(request)),
            FileIo32::Ready(mut receipt) => {
                receipt.cursor = Some((
                    open,
                    offset
                        .checked_add(receipt.bytes.len() as u64)
                        .ok_or(FileError32::InvalidParameter)?,
                ));
                receipt.next = self.stamp()?.1;
                Ok(FileIo32::Ready(receipt))
            }
        }
    }
}
