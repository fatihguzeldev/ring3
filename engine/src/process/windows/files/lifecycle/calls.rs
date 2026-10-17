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
        if self.resume_animation_file_wait(reply.owner, token, reply.frame)? {
            return Ok(());
        }
        if matches!(
            crate::windows::user32::User32Api32::from_id(reply.id),
            Some(
                crate::windows::user32::User32Api32::SetClipboardData
                    | crate::windows::user32::User32Api32::EmptyClipboard
            )
        ) && self.resume_clipboard_file_wait(reply.owner, token, reply.frame)?
        {
            return Ok(());
        }
        if reply.id == crate::windows::WindowsApi32::ExitProcess.id() {
            self.publish_process_exit(self.pending_process_exit.ok_or(HostError::Infrastructure)?)?;
        } else {
            self.execute_pending_guarded_service(reply.owner, token, |engine| {
                if crate::windows::gdi::GdiApi32::from_id(reply.id).is_some() {
                    engine.try_complete_gdi_call(reply.owner, token, reply.id, reply.frame)?;
                } else {
                    engine.try_complete_file_call(reply.owner, token, reply.id, reply.frame)?;
                }
                Ok(())
            })?;
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
        if self.animation_file_waiting() {
            return Err(HostError::Call(CallError::Busy));
        }
        if matches!(
            crate::windows::user32::User32Api32::from_id(reply.id),
            Some(
                crate::windows::user32::User32Api32::SetClipboardData
                    | crate::windows::user32::User32Api32::EmptyClipboard
            )
        ) {
            return Err(HostError::Call(CallError::Busy));
        }
        if reply.id == crate::windows::WindowsApi32::ExitProcess.id() {
            return Err(HostError::Call(CallError::Busy));
        }
        let font_source = self.check_cancel_gdi_font_file(reply.owner, token)?;
        if let Some(request) = reply.request {
            match self.files.as_mut().unwrap().vfs.cancel_request(request) {
                Ok(()) | Err(FileError32::Stale) => {}
                Err(error) => return Err(HostError::File(error)),
            }
        }
        let runtime = self.files.as_mut().unwrap();
        runtime
            .cancel_blocking_file_wait(
                self.memory.as_mut().unwrap(),
                self.synchronization.as_mut().map(|sync| sync.context_mut()),
            )
            .map_err(HostError::File)?;
        runtime
            .cancel_pipe_wait(self.synchronization.as_mut().map(|sync| sync.context_mut()))
            .map_err(HostError::File)?;
        runtime.mappings.cancel_pending();
        runtime.asynchronous.cancel_wait();
        if let Some(wait) = runtime.completion_wait.take() {
            runtime.completion.cancel_wait_for(wait.thread, wait.port);
        }
        runtime.pipe_wait = None;
        runtime.reply = None;
        self.commit_cancel_gdi_font_file(font_source);
        self.abandon_call(key, token)
    }
}
