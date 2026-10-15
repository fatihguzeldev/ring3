use super::super::*;
use crate::process::{EngineInstance, HostError};

impl EngineInstance {
    pub(in crate::process) fn finalize_file_process_exit(&mut self) -> Result<bool, HostError> {
        if self.files.is_none() {
            return Ok(true);
        }
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        let (owner, token, frame) = (pending.owner, pending.token, pending.frame);
        self.checked_pending_call(owner, token)?;
        let request = self
            .files
            .as_mut()
            .unwrap()
            .flush_exit_mappings(self.memory.as_ref().ok_or(HostError::Closed)?)
            .map_err(HostError::File)?;
        if let Some(request) = request {
            self.files.as_mut().unwrap().reply = Some(FileReply32 {
                owner,
                token,
                frame,
                id: crate::windows::WindowsApi32::ExitProcess.id(),
                request: Some(request),
            });
            return Ok(false);
        }
        let files = self.files.as_ref().unwrap();
        let native: crate::files::PreparedFileExit32 =
            files.vfs.prepare_process_exit().map_err(HostError::File)?;
        native.validate(&files.vfs).map_err(HostError::File)?;
        let mappings = files
            .mappings
            .prepare_process_exit(self.memory.as_ref().unwrap())
            .map_err(HostError::File)?;
        let bindings = files
            .asynchronous
            .process_exit_bindings()
            .map_err(HostError::File)?;
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(files.events.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        handles.extend(files.events.iter().map(|(_, handle)| *handle));
        let events = if bindings.is_empty() && handles.is_empty() {
            None
        } else {
            Some(
                self.synchronization
                    .as_ref()
                    .ok_or(HostError::File(FileError32::Unsupported))?
                    .context()
                    .prepare_event_shutdown(&bindings, &handles)
                    .map_err(HostError::Synchronization)?,
            )
        };
        let files = self.files.as_mut().unwrap();
        files
            .mappings
            .commit_process_exit(self.memory.as_mut().unwrap(), mappings);
        if let Some(events) = events {
            self.synchronization
                .as_mut()
                .unwrap()
                .context_mut()
                .commit_event_shutdown(events);
        }
        files.vfs.commit_process_exit(native);
        files.asynchronous.commit_process_exit();
        files.ordinary.terminate();
        files.handles.commit_process_exit();
        files.events.clear();
        files.reply = None;
        Ok(true)
    }
}
