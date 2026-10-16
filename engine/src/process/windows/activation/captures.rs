use super::PreparedActivationCapture32;
use crate::{
    process::{EngineInstance, HostError},
    windows::activation::{ActivationError32 as E, CapturedActivation32},
};
use std::sync::Weak;

pub(super) struct ManagedActivationCapture32 {
    pub(super) handle: u32,
    pub(super) lease: Weak<()>,
}
pub(in crate::process) struct PreparedManagedActivationCapture32 {
    raw: PreparedActivationCapture32,
    token: CapturedActivation32,
}
impl PreparedManagedActivationCapture32 {
    pub(in crate::process) fn token(&self) -> CapturedActivation32 {
        self.token.clone()
    }
}
impl EngineInstance {
    pub(in crate::process) fn prepare_managed_activation_capture(
        &mut self,
    ) -> Result<PreparedManagedActivationCapture32, HostError> {
        self.collect_activation_captures()?;
        let raw = self.prepare_activation_capture()?;
        let token = CapturedActivation32::new(raw.handle());
        if token.handle() != 0 {
            let limit = self
                .user32
                .as_ref()
                .map_or(0, |context| context.activation_capture_limit())
                + self.synchronization.as_ref().map_or(0, |runtime| {
                    // a running callback owns its former registration independently.
                    // one additional receipt prepares a replacement before publication.
                    runtime.context().timer_capture_capacity()
                        + self.threads.managed_activation_capture_capacity()
                        + 1
                });
            let runtime = self.activation.as_mut().ok_or(HostError::Infrastructure)?;
            if runtime.captures.len() >= limit {
                return Err(HostError::Activation(E::NotEnoughMemory));
            }
            runtime
                .captures
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        }
        Ok(PreparedManagedActivationCapture32 { raw, token })
    }
    pub(in crate::process) fn commit_managed_activation_capture(
        &mut self,
        prepared: PreparedManagedActivationCapture32,
    ) {
        let handle = self.commit_activation_capture(prepared.raw);
        if let Some(lease) = prepared.token.lease() {
            self.activation
                .as_mut()
                .unwrap()
                .captures
                .push(ManagedActivationCapture32 { handle, lease });
        }
    }
    pub(in crate::process) fn collect_activation_captures(&mut self) -> Result<(), HostError> {
        if let Some(runtime) = &mut self.synchronization {
            runtime.retire_stale_timer_captures();
        }
        let Some(runtime) = &self.activation else {
            return Ok(());
        };
        let count = runtime
            .captures
            .iter()
            .filter(|capture| capture.lease.strong_count() == 0)
            .count();
        if count == 0 {
            return Ok(());
        }
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(count)
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        handles.extend(
            runtime
                .captures
                .iter()
                .filter(|capture| capture.lease.strong_count() == 0)
                .map(|capture| capture.handle),
        );
        let release = self.prepare_activation_hold_release(&handles)?;
        self.commit_activation_hold_release(release);
        self.activation
            .as_mut()
            .unwrap()
            .captures
            .retain(|capture| capture.lease.strong_count() != 0);
        Ok(())
    }
}
