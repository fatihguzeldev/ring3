use crate::{
    files::FileReadRequest32,
    process::{CallError, EngineInstance, HostError},
};
impl EngineInstance {
    pub fn pending_dinput_file_token(&self) -> Option<u32> {
        let pending = self.dinput.as_ref()?.file_call.as_ref()?;
        self.checked_pending_call(pending.owner, pending.token)
            .ok()?;
        Some(pending.token)
    }
    pub(in crate::process) fn dinput_file_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self.dinput.as_ref().is_some_and(|runtime| {
                runtime
                    .file_call
                    .as_ref()
                    .is_some_and(|pending| pending.token == token)
            })
    }
    pub fn resume_dinput_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let pending = self
            .dinput
            .as_ref()
            .and_then(|runtime| runtime.file_call.as_ref())
            .filter(|pending| pending.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let (owner, gate, frame) = (pending.owner, pending.gate, pending.frame);
        self.checked_pending_call(owner, token)?;
        self.check_windows_callback_capacity()?;
        self.execute_pending_guarded_service(owner, token, |engine| {
            engine.try_complete_dinput_file_call(owner, token, gate, frame)?;
            Ok(())
        })
    }
    pub fn cancel_dinput_file_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let pending = self
            .dinput
            .as_ref()
            .and_then(|runtime| runtime.file_call.as_ref())
            .filter(|pending| pending.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(pending.owner, token)?;
        let lease = pending.lease;
        if let Some(lease) = lease {
            self.close_provider_file(lease)?
        }
        self.dinput.as_mut().unwrap().file_call = None;
        self.abandon_call(key, token)
    }
}
impl super::super::super::DirectInputRuntime32 {
    pub(in crate::process) fn active_file_request(&self) -> Option<FileReadRequest32> {
        self.file_call.as_ref().and_then(|pending| pending.request)
    }
    pub(in crate::process) fn pending_file_owner(
        &self,
    ) -> Option<crate::process::calls::call::PendingOwner> {
        self.file_call.as_ref().map(|pending| pending.owner)
    }
}
