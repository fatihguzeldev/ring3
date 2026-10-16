use super::VersionThreadReply32;
use crate::{
    files::FileReadRequest32,
    process::{CallError, EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingVersionCall32 {
    pub key: u64,
    pub token: u32,
    pub request: FileReadRequest32,
    pub ready: bool,
}
impl EngineInstance {
    pub fn pending_version_call(&self) -> Result<Option<PendingVersionCall32>, HostError> {
        self.memory()?;
        let Some(reply) = self.version.reply.as_ref() else {
            return Ok(None);
        };
        self.checked_pending_call(reply.owner, reply.token)?;
        let Some(request) = reply.request else {
            return Ok(None);
        };
        Ok(Some(PendingVersionCall32 {
            key: self.key,
            token: reply.token,
            request,
            ready: self.provider_file_request_ready(request),
        }))
    }
    pub fn resume_version_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .version
            .reply
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.token != token {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if reply
            .request
            .is_some_and(|r| !self.provider_file_request_ready(r))
        {
            return Err(HostError::Call(CallError::Busy));
        }
        let (owner, api, frame) = (reply.owner, reply.api, reply.frame);
        self.execute_pending_guarded_service(owner, token, |engine| {
            engine.try_complete_version_call(owner, token, api.id(), frame)?;
            Ok(())
        })
    }
    pub(in crate::process) fn version_thread_reply_ready(
        &self,
        reply: &VersionThreadReply32,
    ) -> bool {
        reply
            .request
            .is_none_or(|r| self.provider_file_request_ready(r))
    }
    pub(in crate::process) fn active_version_reply_ready(&self) -> bool {
        self.version
            .reply
            .as_ref()
            .is_some_and(|r| self.version_thread_reply_ready(r))
    }
    pub(in crate::process) fn version_provider_owns_token(&self, token: u32) -> bool {
        self.version.owns_token(token)
    }
}
