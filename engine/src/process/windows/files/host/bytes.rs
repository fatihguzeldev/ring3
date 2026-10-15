use super::*;

impl EngineInstance {
    pub fn pending_file_request(&self) -> Result<Option<FileReadRequest32>, HostError> {
        self.memory()?;
        Ok(self
            .files
            .as_ref()
            .ok_or(HostError::File(FileError32::Unsupported))?
            .vfs
            .pending_request())
    }
    pub fn supply_file_bytes(
        &mut self,
        request: FileReadRequest32,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.files_mut()?
            .vfs
            .supply_bytes(request, offset, bytes)
            .map_err(HostError::File)
    }
    pub fn progress_file_io(&mut self, epoch: u64) -> Result<FileProgress32, HostError> {
        self.memory()?;
        let files = self
            .files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        let outcome = files
            .progress_io(
                self.memory.as_mut().unwrap(),
                self.synchronization
                    .as_mut()
                    .map(|runtime| runtime.context_mut()),
            )
            .map_err(HostError::File)?;
        use super::asynchronous::AsyncProgress32;
        Ok(match outcome {
            AsyncProgress32::Idle => FileProgress32 {
                operation: 0,
                error: 0,
                bytes: 0,
                pending_bytes: false,
            },
            AsyncProgress32::Completed {
                id, error, bytes, ..
            } => FileProgress32 {
                operation: id,
                error,
                bytes,
                pending_bytes: false,
            },
            AsyncProgress32::NeedBytes(_) => FileProgress32 {
                operation: 0,
                error: 0,
                bytes: 0,
                pending_bytes: true,
            },
        })
    }
}

impl EngineInstance {
    pub fn cancel_file_request(&mut self, request: FileReadRequest32) -> Result<(), HostError> {
        self.files_mut()?
            .vfs
            .cancel_request(request)
            .map_err(HostError::File)
    }
}
