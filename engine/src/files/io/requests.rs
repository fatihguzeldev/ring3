use super::*;

impl Vfs32 {
    pub(crate) fn request_ready(&self, request: FileReadRequest32) -> bool {
        !self
            .cache
            .pages
            .iter()
            .any(|page| page.request == request && page.supplied != request.length as usize)
    }
    pub(crate) fn cancel_operation_io(&mut self, operation: u64) {
        if operation == 0 || operation == u64::MAX {
            return;
        }
        self.cancel_owned_io(FileIoOwner32::AsyncOperation(operation));
    }
    pub(crate) fn cancel_guest_thread_io(&mut self, thread: u32) {
        self.cancel_owned_io(FileIoOwner32::GuestThread(thread));
    }
    pub(super) fn cancel_owned_io(&mut self, operation: FileIoOwner32) {
        let reads = &self.cache.stages;
        let writes = &self.pending_writes;
        self.cache.pages.retain(|p| {
            !reads.iter().any(|s| {
                s.owner == operation
                    && (s.waiting == Some(p.request) || s.completed_request == Some(p.request))
            }) && !writes
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
        let index = self.cache.pages.iter().position(|p| p.request == request);
        let staged = self.cache.stages.iter().any(|stage| {
            stage.waiting == Some(request) || stage.completed_request == Some(request)
        }) || self
            .pending_writes
            .iter()
            .any(|stage| stage.waiting == Some(request));
        if index.is_none() && !staged {
            return Err(FileError32::Stale);
        }
        if let Some(index) = index {
            self.cache.pages.swap_remove(index);
        }
        self.cache
            .stages
            .retain(|s| s.waiting != Some(request) && s.completed_request != Some(request));
        self.pending_writes.retain(|s| s.waiting != Some(request));
        Ok(())
    }
}
