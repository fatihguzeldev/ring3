use crate::{process::EngineInstance, windows::synchronization::SynchronizationError32 as Error};

impl EngineInstance {
    pub(in crate::process) fn validate_windows_process_handle(
        &self,
        handle: u32,
        access: u32,
    ) -> Result<(), Error> {
        if handle == u32::MAX {
            return Ok(());
        }
        let process = self
            .synchronization
            .as_ref()
            .ok_or(Error::InvalidHandle)?
            .context()
            .validate_process_access(handle, access)?;
        if process == self.current_windows_process_id() {
            Ok(())
        } else {
            Err(Error::InvalidHandle)
        }
    }
}
