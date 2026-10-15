use crate::process::{CallError, EngineInstance, HostError, calls::call::PendingOwner};
use crate::{
    abi::x86::decode_exit,
    cpu::ExitReason,
    windows::{ProcessContext32, WindowsApi32, WindowsOutcome32},
};

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

    pub fn complete_active_resident_callback_windows_call(
        &mut self,
        key: u64,
        unit_id: u64,
        callback_token: u32,
        inner_token: u32,
    ) -> Result<(), HostError> {
        let owner = self.active_resident_callback_call_owner(key, unit_id, callback_token)?;
        self.complete_windows_call_frame(owner, inner_token)
    }

    fn complete_windows_call_frame(
        &mut self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<(), HostError> {
        let pending = self.checked_pending_call(owner, token)?;
        let frame = pending.frame;
        let saved_exit = pending.exit;
        if self.retry_module_windows_call(owner, token)? {
            return Ok(());
        }
        if self.callback.is_some() && !matches!(owner, PendingOwner::ResidentCallback { .. }) {
            return Err(HostError::Call(CallError::Busy));
        }
        let exit = decode_exit(&saved_exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Infrastructure);
        };
        let api = WindowsApi32::from_id(id).ok_or(HostError::Call(CallError::InvalidRequest))?;
        if self.complete_module_api(api, frame)? {
            return Ok(());
        }
        let thread = self.current_windows_thread()?;
        if matches!(
            api,
            WindowsApi32::SetLastError
                | WindowsApi32::VirtualAlloc
                | WindowsApi32::VirtualFree
                | WindowsApi32::VirtualProtect
        ) {
            self.check_windows_thread_write()?;
        }
        let (outcome, thread) = thread
            .prepare(
                api,
                &frame,
                ProcessContext32 {
                    main_image_base: self
                        .image
                        .map(|image| crate::memory::GuestAddress(image.image_base)),
                },
            )
            .map_err(|_| HostError::Call(CallError::InvalidRequest))?;
        match outcome {
            WindowsOutcome32::Return(result) => {
                let prepared = Self::prepare_completed_call(frame.complete(result))?;
                self.publish_windows_thread(thread)?;
                self.publish_prepared_call(prepared);
            }
            WindowsOutcome32::ExitProcess(code) => {
                self.publish_process_exit(code)?;
            }
            WindowsOutcome32::Allocate { size, permissions } => {
                self.complete_virtual_alloc(frame, thread, size, permissions)?;
            }
            WindowsOutcome32::Release { address } => {
                self.complete_virtual_free(frame, thread, address)?;
            }
            WindowsOutcome32::Protect {
                range,
                permissions,
                old_output,
            } => {
                self.complete_virtual_protect(frame, thread, range, permissions, old_output)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "calls/tests/calls.rs"]
mod windows_tests;

#[cfg(test)]
#[path = "calls/tests/allocation.rs"]
mod windows_allocation_tests;

#[cfg(test)]
#[path = "calls/tests/callback.rs"]
mod windows_callback_tests;
