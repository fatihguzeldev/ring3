#![forbid(unsafe_code)]

use super::{CallError, EngineInstance, HostError, callback::SuspendedCallback};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        callback::{CALLBACK_RECORD_SIZE, CallbackRecord32, encode_callback},
        x86::{EXIT_SIZE, EXIT_VERSION_3, STATE_SIZE, decode_exit, decode_state},
    },
    cpu::{ExitReason, dbt::CompiledRegion},
};

struct PreparedCallbackCode {
    key: u64,
    generation: u32,
    token: u32,
    pc: u32,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
    artifact: CompiledRegion,
    record: CallbackRecord32,
    output: [u8; CALLBACK_RECORD_SIZE],
}

impl EngineInstance {
    pub fn resume_callback_code(
        &mut self,
        key: u64,
        generation: u32,
        callback_token: u32,
        count: u32,
        gate_count: u32,
    ) -> Result<u32, HostError> {
        let prepared =
            self.prepare_callback_code(key, generation, callback_token, count, gate_count)?;
        self.install_callback_code(prepared)
    }

    fn prepare_callback_code(
        &self,
        key: u64,
        generation: u32,
        token: u32,
        count: u32,
        gate_count: u32,
    ) -> Result<PreparedCallbackCode, HostError> {
        let callback = self.callback_code_context(key, generation, token)?;
        Self::check_region_counts(count, gate_count)?;
        let state: [u8; STATE_SIZE] = self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
            .try_into()
            .unwrap();
        let exit: [u8; EXIT_SIZE] = self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap();
        let stopped = decode_state(&state).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if u16::from_le_bytes(exit[4..6].try_into().unwrap()) != EXIT_VERSION_3
            || decode_exit(&exit)
                .map_err(|_| HostError::Call(CallError::InvalidStop))?
                .reason
                != ExitReason::NeedCode
        {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let next_generation = generation
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)?;
        let artifact = self.prepare_artifact(count, gate_count, next_generation)?;
        replacement_admission(&artifact, callback, stopped.eip)?;
        let record = CallbackRecord32 {
            generation: next_generation,
            phase: 1,
            outcome: 0,
            result: 0,
            ..callback.record
        };
        let mut output = [0; CALLBACK_RECORD_SIZE];
        encode_callback(&record, &mut output).map_err(|_| HostError::Infrastructure)?;
        Ok(PreparedCallbackCode {
            key,
            generation,
            token,
            pc: stopped.eip,
            state,
            exit,
            artifact,
            record,
            output,
        })
    }

    fn install_callback_code(&mut self, prepared: PreparedCallbackCode) -> Result<u32, HostError> {
        let callback =
            self.callback_code_context(prepared.key, prepared.generation, prepared.token)?;
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != prepared.state
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != prepared.exit
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        prepared
            .artifact
            .wasm_bytes(self.memory()?)
            .map_err(|_| HostError::CodeInvalidated)?;
        replacement_admission(&prepared.artifact, callback, prepared.pc)?;

        // every fallible check precedes the artifact and continuation publication.
        let generation = prepared.record.generation;
        self.artifact = Some(prepared.artifact);
        self.generation = generation;
        let callback = self.callback.as_mut().unwrap();
        callback.record = prepared.record;
        callback.outer.generation = generation;
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE]
            .copy_from_slice(&prepared.output);
        Ok(generation)
    }

    fn callback_code_context(
        &self,
        key: u64,
        generation: u32,
        token: u32,
    ) -> Result<&SuspendedCallback, HostError> {
        self.guard_artifact(key, generation)?;
        let callback = self
            .callback
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if token == 0 || callback.record.token != token || callback.record.generation != generation
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(callback)
    }
}

fn replacement_admission(
    artifact: &CompiledRegion,
    callback: &SuspendedCallback,
    pc: u32,
) -> Result<(), HostError> {
    let outer_exit =
        decode_exit(&callback.outer.exit).map_err(|_| HostError::Call(CallError::InvalidStop))?;
    let ExitReason::Gate { id } = outer_exit.reason else {
        return Err(HostError::Call(CallError::InvalidStop));
    };
    if !artifact.contains_instruction(pc)
        || !artifact.matches_gate(callback.record.return_pc, callback.record.return_id)
        || !artifact.matches_gate(callback.outer.frame.state().eip, id)
    {
        return Err(HostError::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
#[path = "callback_code_tests.rs"]
mod tests;
