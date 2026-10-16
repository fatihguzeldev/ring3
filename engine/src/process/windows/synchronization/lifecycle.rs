use super::{SynchronizationRuntime32, WindowsWait32};
use crate::{
    process::{CallError, EngineInstance, HostError},
    windows::synchronization::{
        SynchronizationContext32, SynchronizationError32 as Error, SynchronizationLimits32,
    },
};

impl SynchronizationRuntime32 {
    pub(in crate::process) fn context(&self) -> &SynchronizationContext32 {
        &self.context
    }
    pub(in crate::process) fn context_mut(&mut self) -> &mut SynchronizationContext32 {
        &mut self.context
    }
}

impl EngineInstance {
    pub fn configure_synchronization(
        &mut self,
        limits: SynchronizationLimits32,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.synchronization.is_some() || self.pending_call.is_some() || self.callback.is_some()
        {
            return Err(HostError::Synchronization(Error::Busy));
        }
        let (_, now) = self.cpu_clock.monotonic_bounds()?;
        let epoch = crate::process::media::next_session()?;
        let mut context = SynchronizationContext32::new(epoch, limits, now)
            .map_err(HostError::Synchronization)?;
        context
            .set_thread_handle_limit(self.threads.handle_limit())
            .map_err(HostError::Synchronization)?;
        let mut critical = Vec::new();
        critical
            .try_reserve_exact(limits.events as usize)
            .map_err(|_| HostError::Synchronization(Error::Allocation))?;
        let mut timer_captures = Vec::new();
        timer_captures
            .try_reserve_exact(limits.events as usize)
            .map_err(|_| HostError::Synchronization(Error::Allocation))?;
        self.synchronization = Some(SynchronizationRuntime32 {
            context,
            reply: None,
            critical,
            timer_captures,
        });
        Ok(())
    }
    pub fn synchronization(&self) -> Result<&SynchronizationContext32, HostError> {
        self.memory()?;
        self.synchronization
            .as_ref()
            .map(SynchronizationRuntime32::context)
            .ok_or(HostError::Synchronization(Error::Unsupported))
    }
    pub(in crate::process) fn synchronization_mut(
        &mut self,
    ) -> Result<&mut SynchronizationContext32, HostError> {
        self.memory()?;
        self.synchronization
            .as_mut()
            .map(SynchronizationRuntime32::context_mut)
            .ok_or(HostError::Synchronization(Error::Unsupported))
    }
    pub(in crate::process) fn synchronization_provider_owns_token(&self, token: u32) -> bool {
        token != 0
            && self
                .synchronization
                .as_ref()
                .and_then(|runtime| runtime.reply)
                .is_some_and(|reply| reply.token == token)
    }
    pub fn signal_windows_event(&mut self, epoch: u64, handle: u32) -> Result<(), HostError> {
        let context = self.synchronization_mut()?;
        if context.epoch() != epoch {
            return Err(HostError::Synchronization(Error::Stale));
        }
        context
            .set_event(handle)
            .map_err(HostError::Synchronization)
    }
    pub fn reset_windows_event(&mut self, epoch: u64, handle: u32) -> Result<(), HostError> {
        let context = self.synchronization_mut()?;
        if context.epoch() != epoch {
            return Err(HostError::Synchronization(Error::Stale));
        }
        context
            .reset_event(handle)
            .map_err(HostError::Synchronization)
    }
    pub fn pending_windows_wait(&self) -> Option<WindowsWait32> {
        let runtime = self.synchronization.as_ref()?;
        let reply = runtime.reply?;
        self.checked_pending_call(reply.owner, reply.token).ok()?;
        let wait = reply.wait?;
        Some(WindowsWait32 {
            token: reply.token,
            epoch: wait.epoch(),
            wait_id: wait.id(),
            deadline_ns: reply.deadline,
            revision: runtime.context.revision(),
            ready: reply.result.is_some() || runtime.context.wait_status(wait).ok()?.is_some(),
            wake_mask: reply.wake_mask,
            alertable: reply.alertable,
        })
    }
    pub fn resume_windows_wait(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token && reply.wait.is_some())
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        self.checked_pending_call(reply.owner, token)?;
        if reply.alertable && self.has_pending_thread_apc(self.threads.current_id()) {
            return Err(HostError::Call(CallError::Busy));
        }
        if reply.result.is_none()
            && self
                .synchronization
                .as_ref()
                .unwrap()
                .context
                .wait_status(reply.wait.unwrap())
                .map_err(HostError::Synchronization)?
                .is_none()
        {
            return Err(HostError::Call(CallError::Busy));
        }
        self.finish_synchronization_reply()
    }
    pub fn cancel_windows_wait(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        let reply = self
            .synchronization
            .as_ref()
            .and_then(|runtime| runtime.reply)
            .filter(|reply| reply.token == token && reply.wait.is_some())
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if self
            .callback
            .as_ref()
            .is_some_and(|callback| callback.contains_outer_token(token))
        {
            return Err(HostError::Call(CallError::Busy));
        }
        if self
            .pending_call
            .as_ref()
            .is_none_or(|pending| pending.token != token || pending.owner != reply.owner)
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        let runtime = self.synchronization.as_mut().unwrap();
        if reply.result.is_none() {
            runtime
                .context
                .cancel_wait(reply.wait.unwrap())
                .map_err(HostError::Synchronization)?;
        }
        runtime.reply = None;
        self.pending_call = None;
        Ok(())
    }
}
