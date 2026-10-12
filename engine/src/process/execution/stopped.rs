use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state},
    },
    cpu::{
        ExitReason,
        dbt::{CompileLimits, compile_runtime_entry},
    },
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, calls::context::ExtendedCpuState},
};

impl EngineInstance {
    /// prepares the current entry after an ordinary replacement `NeedCode` stop.
    pub fn compile_stopped_entry(&mut self, key: u64, generation: u32) -> Result<u32, HostError> {
        self.guard_artifact(key, generation)?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let arena = self.arena();
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        ExtendedCpuState::capture(arena)?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if exit.reason != ExitReason::NeedCode
            || self
                .artifact
                .as_ref()
                .unwrap()
                .contains_executable(state.eip)
        {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let next_generation = self
            .generation
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)?;
        let artifact = compile_runtime_entry(
            self.memory()?,
            GuestAddress(state.eip),
            CompileLimits::default(),
            key,
            next_generation,
        )
        .map_err(HostError::Compile)?;
        self.artifact = Some(artifact);
        self.generation = next_generation;
        Ok(next_generation)
    }
}

#[cfg(test)]
#[path = "stopped/tests.rs"]
mod tests;
