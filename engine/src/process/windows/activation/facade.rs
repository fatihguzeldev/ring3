use super::{ActivationThreadReply32, ActivationWait32};
use crate::{
    process::{CallError, EngineInstance, HostError},
    windows::activation::ActivationError32 as E,
};

use crate::windows::activation::ComRedirection32;

impl EngineInstance {
    pub(in crate::process) fn active_com_redirection(
        &self,
        clsid: [u8; 16],
    ) -> Result<Option<ComRedirection32>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        let result = runtime
            .native
            .active_com_redirection(self.current_windows_thread_id()?, clsid)
            .map_err(HostError::Activation)?;
        if let Some(result) = &result {
            runtime
                .storage(result.context)
                .map_err(HostError::Activation)?
                .check(self.memory()?)
                .map_err(HostError::Activation)?;
        }
        Ok(result)
    }
    #[cfg(test)]
    pub(in crate::process) fn active_dll_redirection(
        &self,
        name: &[u16],
    ) -> Result<Option<Vec<u16>>, HostError> {
        let Some(runtime) = &self.activation else {
            return Ok(None);
        };
        for handle in runtime
            .native
            .effective_lookup_handles(self.current_windows_thread_id()?)
        {
            if handle != 0 {
                runtime
                    .storage(handle)
                    .map_err(HostError::Activation)?
                    .check(self.memory()?)
                    .map_err(HostError::Activation)?;
            }
        }
        runtime
            .native
            .active_dll_redirection(self.current_windows_thread_id()?, name)
            .map_err(HostError::Activation)
    }
    pub(super) fn query_activation_module_context(
        &self,
        flags: u32,
        handle: u32,
    ) -> Result<u32, E> {
        if flags & !0x8000001c != 0 || (flags & 0x1c).count_ones() > 1 || handle == 0 {
            return Err(E::InvalidParameter);
        }
        let module = self
            .loaded_windows_modules()
            .find(|module| {
                if flags & 8 != 0 {
                    module.base() == handle
                } else {
                    u64::from(handle) >= u64::from(module.base())
                        && u64::from(handle)
                            < u64::from(module.base()) + u64::from(module.image_size())
                }
            })
            .ok_or(E::Win32(126))?;
        Ok(self
            .activation
            .as_ref()
            .and_then(|runtime| {
                runtime
                    .module_contexts
                    .iter()
                    .find(|(id, _)| *id == module.id())
                    .map(|(_, handle)| *handle)
            })
            .unwrap_or(0))
    }
    pub(in crate::process) fn activation_provider_owns_token(&self, token: u32) -> bool {
        self.activation
            .as_ref()
            .is_some_and(|runtime| runtime.provider_owns_token(token))
    }
    pub(in crate::process) fn activation_thread_reply_ready(
        &self,
        reply: &ActivationThreadReply32,
    ) -> bool {
        reply
            .request
            .is_none_or(|request| self.provider_file_request_ready(request))
    }
    pub(in crate::process) fn active_activation_reply_ready(&self) -> bool {
        self.activation
            .as_ref()
            .and_then(|runtime| runtime.reply.as_ref())
            .is_some_and(|reply| self.activation_thread_reply_ready(reply))
    }
    pub fn pending_activation_call(&self) -> Option<ActivationWait32> {
        let reply = self.activation.as_ref()?.reply.as_ref()?;
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(ActivationWait32 {
            epoch: self.key,
            token: reply.token,
            ready: self.activation_thread_reply_ready(reply),
        })
    }
    pub fn resume_activation_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .activation
            .as_ref()
            .and_then(|runtime| runtime.reply.as_ref())
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let (owner, frame, id) = (reply.owner, reply.frame, reply.api.id());
        self.execute_pending_guarded_service(owner, token, |engine| {
            engine.try_complete_activation_call(owner, token, id, frame)?;
            Ok(())
        })
    }
}
