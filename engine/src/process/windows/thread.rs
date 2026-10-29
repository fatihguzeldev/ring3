use crate::{
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
    windows::ThreadState32,
};

impl EngineInstance {
    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn execute_external_direct_windows_gate(
        &mut self,
        id: u32,
        frame: crate::windows::CallFrame32,
    ) -> Result<u32, HostError> {
        use crate::windows::{
            ProcessContext32, WindowsApi32, WindowsOutcome32, thread::ThreadApi32,
        };
        if id == ThreadApi32::TlsGetValue.id() {
            self.check_windows_thread_write()?;
            let thread = self.current_windows_thread()?;
            if self.module_teb_address().is_some() {
                self.memory()?
                    .ensure_transaction_versions(1)
                    .map_err(HostError::Memory)?;
            }
            let value = self.get_windows_tls(frame.arguments()[0])?;
            self.publish_windows_thread(thread.with_last_error(0))?;
            return Ok(value);
        }
        let Some(api @ (WindowsApi32::GetLastError | WindowsApi32::SetLastError)) =
            WindowsApi32::from_id(id)
        else {
            return self.execute_external_direct_d3d8_gate(id, frame);
        };
        let thread = self.current_windows_thread()?;
        if api == WindowsApi32::SetLastError {
            self.check_windows_thread_write()?;
        }
        let (outcome, thread) = thread
            .prepare(api, &frame, ProcessContext32::default())
            .map_err(|_| HostError::Call(crate::process::CallError::InvalidRequest))?;
        let WindowsOutcome32::Return(value) = outcome else {
            return Err(HostError::Infrastructure);
        };
        self.publish_windows_thread(thread)?;
        Ok(value)
    }

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
        if self.defer_guard_service_thread(thread) {
            return Ok(());
        }
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
