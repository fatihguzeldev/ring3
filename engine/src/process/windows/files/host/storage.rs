use super::*;

impl EngineInstance {
    // guest exit retains save data; close releases the storage context.
    pub(in crate::process::windows::files) fn files_storage(
        &self,
    ) -> Result<&FileRuntime32, HostError> {
        if !self.is_open() {
            return Err(HostError::Closed);
        }
        self.files
            .as_ref()
            .ok_or(HostError::File(FileError32::Unsupported))
    }
    pub(in crate::process::windows::files) fn files_storage_mut(
        &mut self,
    ) -> Result<&mut FileRuntime32, HostError> {
        if !self.is_open() {
            return Err(HostError::Closed);
        }
        self.files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))
    }
}
