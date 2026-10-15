use super::super::*;
use crate::process::{CallError, EngineInstance, HostError};

impl EngineInstance {
    pub(in crate::process) fn files_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self
                .files
                .as_ref()
                .and_then(|runtime| runtime.reply)
                .is_some_and(|reply| reply.token == token)
    }
    pub fn pending_file_call(&self) -> Option<FileWait32> {
        let runtime = self.files.as_ref()?;
        let reply = runtime.reply?;
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(FileWait32 {
            epoch: runtime.vfs.epoch(),
            token: reply.token,
        })
    }
    pub fn resume_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .files
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if reply.id == crate::windows::WindowsApi32::ExitProcess.id() {
            self.publish_process_exit(reply.frame.arguments()[0])?;
        } else {
            self.try_complete_file_call(reply.owner, token, reply.id, reply.frame)?;
        }
        Ok(())
    }
    pub fn cancel_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .files
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, token)?;
        if reply.id == crate::windows::WindowsApi32::ExitProcess.id() {
            return Err(HostError::Call(CallError::Busy));
        }
        if let Some(request) = reply.request {
            match self.files.as_mut().unwrap().vfs.cancel_request(request) {
                Ok(()) | Err(FileError32::Stale) => {}
                Err(error) => return Err(HostError::File(error)),
            }
        }
        let runtime = self.files.as_mut().unwrap();
        runtime.mappings.cancel_pending();
        runtime.asynchronous.cancel_wait();
        runtime.reply = None;
        self.abandon_call(key, token)
    }
}
