use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state},
    },
    cpu::{
        ExitReason,
        dbt::{
            CompileLimits, RegistryError, RegistryLimits, ResidentRegistry, UnitId,
            compile_runtime_entry,
        },
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
        let cs = self.code_fetch_context()?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if exit.reason != ExitReason::NeedCode
            || self
                .artifact
                .as_ref()
                .unwrap()
                .contains_material_in(state.eip, cs.decode_key())
            || self
                .artifact
                .as_ref()
                .unwrap()
                .context_fault_in(state.eip, cs.decode_key())
                .is_some_and(|fault| !cs.permits(fault.pc, fault.attempted_length))
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
            cs,
            self.system_register_bank,
        )
        .map_err(HostError::Compile)?;
        self.artifact = Some(artifact);
        self.generation = next_generation;
        Ok(next_generation)
    }
    /// captures only an ordinary resident code miss; strict resident compilers stay strict.
    pub fn compile_stopped_resident_entry(&mut self, key: u64) -> Result<UnitId, HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let arena = self.arena();
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        ExtendedCpuState::capture(arena)?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if exit.reason != ExitReason::NeedCode {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        let cs = self.code_fetch_context()?;
        let pc = GuestAddress(state.eip);
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        if let Some(registry) = &self.resident {
            match registry.lookup_bound(memory, pc, cs) {
                Ok(_) => return Err(HostError::Call(CallError::InvalidStop)),
                Err(RegistryError::NotFound { .. }) => {}
                Err(error) => return Err(HostError::Resident(error)),
            }
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        if let Some(registry) = self.resident.as_mut() {
            registry
                .compile_runtime_bound(
                    memory,
                    pc,
                    CompileLimits::default(),
                    key,
                    cs,
                    self.system_register_bank,
                )
                .map_err(HostError::Resident)
        } else {
            let mut registry = ResidentRegistry::new(memory, RegistryLimits::default())
                .map_err(HostError::Resident)?;
            let id = registry
                .compile_runtime_bound(
                    memory,
                    pc,
                    CompileLimits::default(),
                    key,
                    cs,
                    self.system_register_bank,
                )
                .map_err(HostError::Resident)?;
            self.resident = Some(registry);
            Ok(id)
        }
    }
}

#[cfg(test)]
#[path = "stopped/tests.rs"]
mod tests;
