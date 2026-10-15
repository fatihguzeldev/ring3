use super::*;

impl Vfs32 {
    pub(crate) fn cancel_operation_io(&mut self, operation: u64) {
        if operation == 0 || operation == u64::MAX {
            return;
        }
        let reads = &self.cache.stages;
        let writes = &self.pending_writes;
        self.cache.pages.retain(|p| {
            !reads
                .iter()
                .any(|s| s.owner == operation && s.waiting == Some(p.request))
                && !writes
                    .iter()
                    .any(|s| s.owner == operation && s.waiting == Some(p.request))
        });
        self.cache.stages.retain(|s| s.owner != operation);
        self.pending_writes.retain(|s| s.owner != operation);
    }
    pub(crate) fn cancel_request(&mut self, request: FileReadRequest32) -> Result<(), FileError32> {
        if request.epoch != self.epoch {
            return Err(FileError32::Stale);
        }
        let index = self
            .cache
            .pages
            .iter()
            .position(|p| p.request == request)
            .ok_or(FileError32::Stale)?;
        self.cache.pages.swap_remove(index);
        self.cache.stages.retain(|s| s.waiting != Some(request));
        self.pending_writes.retain(|s| s.waiting != Some(request));
        Ok(())
    }
}
