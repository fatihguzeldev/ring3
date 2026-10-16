use crate::process::{CallError, EngineInstance, HostError};
impl super::RegistryRuntime32 {
    pub(in crate::process) fn thread_reply_ready(&self, reply: &super::RegistryReply32) -> bool {
        match reply.wait {
            super::context::RegistryWaitKind32::Flush => self
                .native
                .can_flush(reply.frame.arguments()[0])
                .unwrap_or(true),
            super::context::RegistryWaitKind32::Notify(id) => self.watch_ready(id),
        }
    }
    pub(in crate::process) fn active_thread_reply_ready(&self) -> bool {
        self.reply
            .as_ref()
            .is_some_and(|reply| self.thread_reply_ready(reply))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryWait32 {
    pub epoch: u64,
    pub token: u32,
    pub ready: bool,
}
impl EngineInstance {
    pub(in crate::process) fn registry_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self
                .registry
                .as_ref()
                .and_then(|runtime| runtime.reply)
                .is_some_and(|reply| reply.token == token)
    }
    pub fn pending_registry_call(&self) -> Option<RegistryWait32> {
        let runtime = self.registry.as_ref()?;
        let reply = runtime.reply?;
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        Some(RegistryWait32 {
            epoch: runtime.native.epoch(),
            token: reply.token,
            ready: runtime.thread_reply_ready(&reply),
        })
    }
    pub fn resume_registry_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .registry
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, token)?;
        let runtime = self.registry.as_ref().unwrap();
        let result = match reply.wait {
            super::context::RegistryWaitKind32::Flush => {
                match runtime.native.can_flush(reply.frame.arguments()[0]) {
                    Ok(false) => return Err(HostError::Call(CallError::Busy)),
                    Ok(true) => 0,
                    Err(error) => error.win32(),
                }
            }
            super::context::RegistryWaitKind32::Notify(id) => {
                if !runtime.watch_ready(id) {
                    return Err(HostError::Call(CallError::Busy));
                }
                0
            }
        };
        let prepared = Self::prepare_completed_call(reply.frame.complete(result))?;
        let runtime = self.registry.as_mut().unwrap();
        if let super::context::RegistryWaitKind32::Notify(id) = reply.wait {
            runtime.finish_watch(id);
        }
        runtime.reply = None;
        self.publish_prepared_call(prepared);
        Ok(())
    }
}
