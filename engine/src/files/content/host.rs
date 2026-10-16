use crate::files::*;

impl Vfs32 {
    pub(crate) fn pending_request(&self) -> Option<FileReadRequest32> {
        self.cache
            .pages
            .iter()
            .find(|p| p.supplied < p.request.length as usize)
            .map(|p| p.request)
    }
    pub(crate) fn supply_bytes(
        &mut self,
        request: FileReadRequest32,
        chunk_offset: u32,
        bytes: &[u8],
    ) -> Result<(), FileError32> {
        if request.epoch != self.epoch
            || self.node(request.file).map(|n| n.content_revision) != Ok(request.revision)
        {
            return Err(FileError32::Stale);
        }
        self.cache.supply(request, chunk_offset, bytes)
    }
    pub(crate) fn overlay_entry(
        &self,
        index: usize,
    ) -> Result<Option<OverlayEntry32>, FileError32> {
        let mut current = 0;
        for entry in &self.entries {
            let node = self.node(entry.file)?;
            if node.dirty {
                if current == index {
                    return Ok(Some(OverlayEntry32 {
                        epoch: self.epoch,
                        revision: self.revision,
                        path: entry.path.clone(),
                        info: Some(node.info()),
                        kind: if node.directory() {
                            OverlayKind32::Directory
                        } else {
                            OverlayKind32::File
                        },
                    }));
                }
                current += 1;
            }
        }
        Ok(self
            .deleted
            .get(index.saturating_sub(current))
            .filter(|_| index >= current)
            .map(|path| OverlayEntry32 {
                epoch: self.epoch,
                revision: self.revision,
                path: path.path.clone(),
                info: None,
                kind: OverlayKind32::Deleted,
            }))
    }
    pub(crate) fn read_overlay(
        &mut self,
        file: FileId,
        revision: u64,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<PreparedRead32>, FileError32> {
        if revision != self.revision {
            return Err(FileError32::Stale);
        }
        self.read_at_owned(
            crate::files::FileIoOwner32::HostExport,
            file,
            offset,
            length,
        )
    }
    pub(crate) fn acknowledge_overlay(
        &mut self,
        epoch: u64,
        revision: u64,
    ) -> Result<(), FileError32> {
        self.validate(epoch, revision)?;
        for node in self.nodes.iter_mut().flatten() {
            node.dirty = false;
        }
        self.deleted.clear();
        Ok(())
    }
}
