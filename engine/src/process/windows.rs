use super::{CallError, EngineInstance, HostError, call::PendingOwner};
use crate::{abi::x86::decode_exit, cpu::ExitReason, windows::WindowsApi32};

impl EngineInstance {
    pub fn complete_windows_call(
        &mut self,
        key: u64,
        generation: u32,
        token: u32,
    ) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        self.complete_windows_call_frame(PendingOwner::Replacement(generation), token)
    }

    pub fn complete_resident_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
        token: u32,
    ) -> Result<(), HostError> {
        self.guard_resident_unit(key, unit_id)?;
        self.complete_windows_call_frame(PendingOwner::Resident(unit_id), token)
    }

    fn complete_windows_call_frame(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let pending = self.checked_pending_call(owner, token)?;
        if self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Infrastructure);
        };
        let api = WindowsApi32::from_id(id).ok_or(HostError::Call(CallError::InvalidRequest))?;
        let (result, thread) = self
            .windows_thread
            .prepare(api, &pending.frame)
            .map_err(|_| HostError::Call(CallError::InvalidRequest))?;
        let state = pending.frame.complete(result);
        self.publish_completed_call(state)?;
        self.windows_thread = thread;
        Ok(())
    }
}
