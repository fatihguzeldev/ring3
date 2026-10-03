#![forbid(unsafe_code)]

use super::{CallError, EngineInstance, HostError};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PAGE_SIZE, PageRange, Permissions},
};

impl EngineInstance {
    pub fn start_loaded_image(&mut self, stack_base: u32, pages: u32) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let image = self.image.ok_or(HostError::InvalidRequest)?;
        if self.image_started || self.call_token != 0 {
            return Err(HostError::InvalidRequest);
        }

        let mut state_bytes = [0; STATE_SIZE];
        let mut exit_bytes = [0; EXIT_SIZE];
        encode_state(&State32::default(), &mut state_bytes)
            .map_err(|_| HostError::Infrastructure)?;
        encode_exit(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::Budget,
            },
            &mut exit_bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != state_bytes
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != exit_bytes
        {
            return Err(HostError::InvalidRequest);
        }

        let stack_end = u64::from(stack_base) + u64::from(pages) * u64::from(PAGE_SIZE);
        if !stack_base.is_multiple_of(PAGE_SIZE)
            || !(1..=4096).contains(&pages)
            || stack_end > 1 << 32
        {
            return Err(HostError::InvalidRequest);
        }
        let image_start = u64::from(image.image_base);
        let image_end = image_start + u64::from(image.image_size);
        if u64::from(stack_base) < image_end && image_start < stack_end {
            return Err(HostError::InvalidRequest);
        }
        let range = PageRange::new(GuestAddress(stack_base), pages)
            .map_err(|_| HostError::InvalidRequest)?;
        self.memory()?
            .fetch(GuestAddress(image.entry_point), &mut [0; 1])
            .map_err(HostError::Memory)?;
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }

        let mut state = State32::default();
        state.registers[4] = stack_end as u32;
        state.eip = image.entry_point;
        encode_state(&state, &mut state_bytes).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit_bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;

        self.memory
            .as_mut()
            .ok_or(HostError::Closed)?
            .map_zeroed(range, Permissions::READ_WRITE)
            .map_err(HostError::Memory)?;
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&state_bytes);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&exit_bytes);
        self.image_started = true;
        Ok(())
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;
