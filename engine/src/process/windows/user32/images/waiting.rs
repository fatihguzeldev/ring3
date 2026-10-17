use super::{super::threads::User32ThreadReply32, ImageReply32};
use crate::{
    files::FileReadRequest32,
    process::{CallError, EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingUser32ImageCall32 {
    pub key: u64,
    pub token: u32,
    pub request: FileReadRequest32,
    pub ready: bool,
}

impl EngineInstance {
    pub fn pending_user32_image_call(&self) -> Result<Option<PendingUser32ImageCall32>, HostError> {
        self.memory()?;
        let Some(reply) = self
            .user32
            .as_ref()
            .and_then(|context| context.images.reply.as_ref())
        else {
            return Ok(None);
        };
        self.checked_pending_call(reply.owner, reply.token)?;
        let Some(request) = reply.pending else {
            return Ok(None);
        };
        Ok(Some(PendingUser32ImageCall32 {
            key: self.key,
            token: reply.token,
            request,
            ready: self.provider_file_request_ready(request),
        }))
    }
    pub fn resume_user32_image_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .user32
            .as_ref()
            .and_then(|context| context.images.reply.as_ref())
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, reply.token)?;
        if reply.token != token {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if reply
            .pending
            .is_some_and(|request| !self.provider_file_request_ready(request))
        {
            return Err(HostError::Call(CallError::Busy));
        }
        let (owner, api, frame) = (reply.owner, reply.api, reply.frame);
        self.execute_pending_guarded_service(owner, token, |engine| {
            engine.try_complete_user32_image_call(owner, token, api.id(), frame)?;
            Ok(())
        })
    }
    pub(in crate::process) fn user32_thread_reply_ready(
        &self,
        reply: &User32ThreadReply32,
    ) -> bool {
        if let Some(image) = reply.image_reply() {
            return self.user32_image_reply_ready(image);
        }
        self.user32
            .as_ref()
            .is_some_and(|context| context.thread_reply_ready(reply))
    }
    pub(in crate::process) fn ready_user32_image_reply_token(&self) -> Option<u32> {
        self.user32
            .as_ref()
            .and_then(|context| context.images.reply.as_ref())
            .filter(|reply| self.user32_image_reply_ready(reply))
            .map(|reply| reply.token)
    }
    fn user32_image_reply_ready(&self, reply: &ImageReply32) -> bool {
        reply
            .pending
            .is_none_or(|request| self.provider_file_request_ready(request))
    }
    pub(in crate::process) fn user32_image_provider_owns_token(&self, token: u32) -> bool {
        self.user32
            .as_ref()
            .and_then(|context| context.images.reply.as_ref())
            .is_some_and(|reply| reply.token == token)
    }
}
