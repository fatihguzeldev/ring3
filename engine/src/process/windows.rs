use super::{CallError, EngineInstance, HostError, call::PendingOwner};
use crate::{
    abi::{
        arena::EXIT_OFFSET,
        x86::{EXIT_SIZE, decode_exit, encode_exit_v4},
    },
    cpu::{ExecutionExit, ExitReason},
    memory::{GuestAddress, MemoryError, PAGE_SIZE, PageRange, Permissions},
    windows::{CallFrame32, ProcessContext32, ThreadState32, WindowsApi32, WindowsOutcome32},
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
        if self.callback.is_some() && !matches!(owner, PendingOwner::ResidentCallback { .. }) {
            return Err(HostError::Call(CallError::Busy));
        }
        let exit = decode_exit(&pending.exit).map_err(|_| HostError::Infrastructure)?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Infrastructure);
        };
        let api = WindowsApi32::from_id(id).ok_or(HostError::Call(CallError::InvalidRequest))?;
        if matches!(owner, PendingOwner::ResidentCallback { .. })
            && api == WindowsApi32::ExitProcess
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
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
            WindowsOutcome32::Allocate { size } => {
                let frame = pending.frame;
                self.complete_virtual_alloc(frame, thread, size)?;
            }
        }
        Ok(())
    }

    fn complete_virtual_alloc(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        size: u32,
    ) -> Result<(), HostError> {
        let bounds = PageRange::new(GuestAddress(0x1000_0000), 0x6000_0000 / PAGE_SIZE)
            .map_err(HostError::Memory)?;
        let planned = self
            .memory()?
            .find_free_range(bounds, size.div_ceil(PAGE_SIZE), 65536);
        let range = match planned {
            Ok(range) => range,
            Err(MemoryError::Capacity) => {
                let prepared = Self::prepare_completed_call(frame.complete(0))?;
                self.publish_prepared_call(prepared);
                self.windows_thread = thread.allocation_failed();
                return Ok(());
            }
            Err(error) => return Err(HostError::Memory(error)),
        };
        let address = range.first as u32 * PAGE_SIZE;
        let prepared = Self::prepare_completed_call(frame.complete(address))?;
        self.memory
            .as_mut()
            .ok_or(HostError::Closed)?
            .map_zeroed(range, Permissions::READ_WRITE)
            .map_err(HostError::Memory)?;
        self.publish_prepared_call(prepared);
        self.windows_thread = thread;
        Ok(())
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod windows_tests;

#[cfg(test)]
#[path = "windows_allocation_tests.rs"]
mod windows_allocation_tests;

#[cfg(test)]
#[path = "windows_callback_tests.rs"]
mod windows_callback_tests;
