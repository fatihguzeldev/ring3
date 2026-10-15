use super::*;

impl Vfs32 {
    pub(crate) fn prepare_resize(
        &self,
        file: FileId,
        size: u64,
    ) -> Result<PreparedResize32, FileError32> {
        let node = self.node(file)?;
        if node.directory() {
            return Err(FileError32::AccessDenied);
        }
        let (revision, next) = self.stamp()?;
        Ok(PreparedResize32 {
            epoch: self.epoch,
            revision,
            next,
            file,
            size,
            content_revision: node
                .content_revision
                .checked_add(1)
                .ok_or(FileError32::Exhausted)?,
        })
    }
    pub(crate) fn commit_resize(&mut self, prepared: PreparedResize32) {
        prepared
            .validate(self)
            .expect("prepared file resize authority");
        let now = self.now;
        let node = self.node_mut(prepared.file).unwrap();
        let old = node.content.pages.len();
        node.content.pages.retain(|p| p.offset < prepared.size);
        if let Some(page) = node.content.pages.last_mut() {
            if page.offset < prepared.size && prepared.size - page.offset < FILE_PAGE_BYTES as u64 {
                page.bytes[(prepared.size - page.offset) as usize..].fill(0);
            }
        }
        let removed = old - node.content.pages.len();
        node.size = prepared.size;
        node.content.visible = node.content.visible.min(prepared.size);
        node.content_revision = prepared.content_revision;
        node.dirty = true;
        node.times.written = now;
        node.changed = now;
        node.attributes = (node.attributes & !ATTR_NORMAL) | ATTR_ARCHIVE;
        self.overlay_pages -= removed;
        self.cache.invalidate(prepared.file);
        self.pending_writes.retain(|s| s.file != prepared.file);
        self.revision = prepared.next;
    }
    pub(crate) fn prepare_seek(
        &self,
        open: OpenFileId,
        distance: i64,
        origin: FileSeek32,
    ) -> Result<PreparedSeek32, FileError32> {
        let description = self.open(open)?;
        if self.node(description.file)?.directory() {
            return Err(FileError32::AccessDenied);
        }
        description
            .cursor_revision
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let base = match origin {
            FileSeek32::Start => 0,
            FileSeek32::Current => description.position,
            FileSeek32::End => self.node(description.file)?.size,
        };
        let position = if distance < 0 {
            base.checked_sub(distance.unsigned_abs())
                .ok_or(FileError32::NegativeSeek)?
        } else {
            base.checked_add(distance as u64)
                .ok_or(FileError32::InvalidParameter)?
        };
        let (revision, next) = self.stamp()?;
        Ok(PreparedSeek32 {
            epoch: self.epoch,
            revision,
            next,
            open,
            position,
        })
    }
    pub(crate) fn commit_seek(&mut self, prepared: PreparedSeek32) -> u64 {
        prepared
            .validate(self)
            .expect("prepared file seek authority");
        let description = self.open_mut(prepared.open).unwrap();
        description.position = prepared.position;
        description.cursor_revision += 1;
        self.revision = prepared.next;
        prepared.position
    }
}
