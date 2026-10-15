use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::ThreadState32,
};

impl EngineInstance {
    pub(super) fn current_windows_thread(&self) -> Result<ThreadState32, HostError> {
        let Some(teb) = self.module_teb_address() else {
            return Ok(self.windows_thread);
        };
        let mut bytes = [0; 4];
        self.memory()?
            .read(GuestAddress(teb + 0x34), &mut bytes)
            .map_err(HostError::Memory)?;
        Ok(self
            .windows_thread
            .with_last_error(u32::from_le_bytes(bytes)))
    }

    pub(super) fn check_windows_thread_write(&self) -> Result<(), HostError> {
        if let Some(teb) = self.module_teb_address() {
            self.memory()?
                .check_span(GuestAddress(teb + 0x34), 4, Access::Write)
                .map_err(HostError::Memory)?;
        }
        Ok(())
    }

    pub(super) fn publish_windows_thread(
        &mut self,
        thread: ThreadState32,
    ) -> Result<(), HostError> {
        if let Some(teb) = self.module_teb_address() {
            let address = GuestAddress(teb + 0x34);
            let mut current = [0; 4];
            self.memory()?
                .read(address, &mut current)
                .map_err(HostError::Memory)?;
            if u32::from_le_bytes(current) != thread.last_error() {
                self.memory
                    .as_mut()
                    .ok_or(HostError::Closed)?
                    .write(address, &thread.last_error().to_le_bytes())
                    .map_err(HostError::Memory)?;
            }
        }
        self.windows_thread = thread;
        Ok(())
    }
}
