use super::{
    MAX_REPLIES, ReplyStage32, WindowsCallback32, WindowsCallbackProvider32, WindowsReply32,
    validate_request,
};
use crate::process::{
    CallError, EngineInstance, HostError,
    calls::{call::PendingOwner, callback::SuspendedRecord},
};

impl EngineInstance {
    pub(in crate::process) fn validate_windows_provider_owner(
        &self,
        owner: PendingOwner,
    ) -> Result<(), HostError> {
        let Some(callback) = self.callback.as_ref() else {
            return Ok(());
        };
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::Busy))?;
        if pending.owner != owner || callback.contains_outer_token(pending.token) {
            return Err(HostError::Call(CallError::Busy));
        }
        match (callback.record, owner) {
            (SuspendedRecord::Replacement(record), PendingOwner::Replacement(generation))
                if record.generation == generation =>
            {
                Ok(())
            }
            (
                SuspendedRecord::Resident {
                    record,
                    authorized: true,
                    active_unit_id,
                },
                PendingOwner::ResidentCallback {
                    unit_id,
                    callback_token,
                },
            ) if active_unit_id == unit_id && record.token == callback_token => Ok(()),
            _ => Err(HostError::Call(CallError::Busy)),
        }
    }

    pub(in crate::process::windows) fn reserve_windows_reply(&mut self) -> Result<(), HostError> {
        let retained = self.modules.parked_windows_reply_count();
        if self.windows_replies.len() + retained >= MAX_REPLIES {
            return Err(HostError::Call(CallError::Busy));
        }
        self.windows_replies
            .try_reserve_exact(MAX_REPLIES - self.windows_replies.len())
            .map_err(|_| HostError::Infrastructure)
    }

    pub(in crate::process) fn check_windows_callback_capacity(&mut self) -> Result<(), HostError> {
        self.memory()?;
        if self.windows_replies.len() + self.modules.parked_windows_reply_count() >= MAX_REPLIES - 1
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.reserve_windows_reply()
    }

    pub(in crate::process::windows) fn validate_new_windows_reply(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let Some(reply) = self.windows_replies.last() else {
            return Ok(());
        };
        if reply.owner == owner && reply.token == token {
            return Err(HostError::Call(CallError::Busy));
        }
        let current = self.callback.as_ref().map(|callback| callback.token());
        if !matches!(reply.stage,ReplyStage32::Resident { token,.. }|ReplyStage32::Replacement { token,.. } if Some(token)==current)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub(in crate::process) fn park_d3d8_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if request.provider != WindowsCallbackProvider32::D3D8 {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }

    pub(in crate::process) fn park_media_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if !matches!(
            request.provider,
            WindowsCallbackProvider32::DirectSound | WindowsCallbackProvider32::DirectInput
        ) {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }

    pub(in crate::process) fn park_file_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if request.provider != WindowsCallbackProvider32::Files {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }
    pub(in crate::process) fn park_resource_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if request.provider != WindowsCallbackProvider32::Resources {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }
    pub(in crate::process) fn park_nls_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if request.provider != WindowsCallbackProvider32::Nls {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }
    pub(in crate::process) fn park_ole32_callback(
        &mut self,
        request: WindowsCallback32,
    ) -> Result<(), HostError> {
        if request.provider != WindowsCallbackProvider32::Ole32 {
            return Err(HostError::InvalidRequest);
        }
        self.park_windows_callback(request)
    }
    fn park_windows_callback(&mut self, request: WindowsCallback32) -> Result<(), HostError> {
        validate_request(request)?;
        self.check_windows_callback_capacity()?;
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let (owner, token, frame) = (pending.owner, pending.token, pending.frame);
        self.validate_windows_provider_owner(owner)?;
        self.validate_new_windows_reply(owner, token)?;
        self.checked_pending_call(owner, token)?;
        if self
            .windows_replies
            .last()
            .is_some_and(|reply| reply.owner == owner && reply.token == token)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.windows_replies.push(WindowsReply32 {
            owner,
            token,
            frame,
            provider: request.provider,
            continuation: request.continuation,
            last_error: None,
            stage: ReplyStage32::Ready(request),
            activation: None,
        });
        Ok(())
    }

    pub(in crate::process) fn windows_provider_owns_callback_token(&self, token: u32) -> bool {
        token!=0&&self.windows_replies.iter().any(|reply|matches!(reply.stage,ReplyStage32::Resident { token:owned,.. }|ReplyStage32::Replacement { token:owned,.. } if owned==token))
    }

    pub(in crate::process) fn windows_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && (self
                .windows_replies
                .iter()
                .any(|reply| reply.token == token)
                || self.synchronization_provider_owns_token(token)
                || self.files_provider_owns_token(token)
                || self.registry_provider_owns_token(token)
                || self.windows_fls_provider_owns_token(token)
                || self.dinput_file_provider_owns_token(token)
                || self.activation_provider_owns_token(token)
                || self.version_provider_owns_token(token)
                || self.ole32_provider_owns_token(token)
                || self.module_source_provider_owns_token(token)
                || self.winmm_provider_owns_token(token))
    }

    pub fn pending_windows_callback(&self) -> Option<WindowsCallback32> {
        let reply = self.windows_replies.last()?;
        let ReplyStage32::Ready(request) = reply.stage else {
            return None;
        };
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(request)
    }
}
