use super::{CallError, EngineInstance, HostError, call::PendingOwner};
use crate::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{EXIT_SIZE, decode_exit, encode_exit_v4},
    },
    cpu::{ExecutionExit, ExitReason},
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
        let (outcome, thread) = self
            .windows_thread
            .prepare(
                api,
                &pending.frame,
                ProcessContext32 {
                    main_image_base: self
                        .image
                        .map(|image| crate::memory::GuestAddress(image.image_base)),
                },
            )
            .map_err(|_| HostError::Call(CallError::InvalidRequest))?;
        match outcome {
            WindowsOutcome32::Return(result) => {
                let state = pending.frame.complete(result);
                self.publish_completed_call(state)?;
                self.windows_thread = thread;
            }
            WindowsOutcome32::ExitProcess(code) => {
                let mut bytes = [0; EXIT_SIZE];
                encode_exit_v4(
                    &ExecutionExit {
                        retired: 0,
                        reason: ExitReason::ProcessExited { code },
                    },
                    &mut bytes,
                )
                .map_err(|_| HostError::Infrastructure)?;
                self.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                    .copy_from_slice(&bytes);
                self.exit_code = Some(code);
                self.pending_call = None;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod windows_tests;
