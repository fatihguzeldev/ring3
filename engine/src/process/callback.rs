#![forbid(unsafe_code)]

use super::{
    CallError, EngineInstance, HostError,
    call::{PendingCall, PendingOwner},
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        callback::{CALLBACK_RECORD_SIZE, CallbackRecord32, encode_callback},
        resident_callback::ResidentCallbackRecord32,
        x86::{
            EXIT_SIZE, STATE_SIZE, X87_SIZE, decode_exit, decode_state, decode_x87, encode_exit_v3,
            encode_state,
        },
    },
    cpu::{ExecutionExit, ExitReason},
    windows::{CallbackFrame32, FrameError, MAX_STACK_WORDS},
};

pub(super) struct SuspendedCallback {
    pub(super) record: SuspendedRecord,
    pub(super) outer: PendingCall,
}

#[derive(Clone, Copy)]
pub(super) enum SuspendedRecord {
    Replacement(CallbackRecord32),
    Resident {
        record: ResidentCallbackRecord32,
        authorized: bool,
        active_unit_id: u64,
    },
}

impl SuspendedCallback {
    pub(super) fn token(&self) -> u32 {
        match self.record {
            SuspendedRecord::Replacement(record) => record.token,
            SuspendedRecord::Resident { record, .. } => record.token,
        }
    }

    pub(super) fn is_resident(&self) -> bool {
        matches!(self.record, SuspendedRecord::Resident { .. })
    }

    pub(super) fn authorized_resident_record(&self) -> Option<ResidentCallbackRecord32> {
        match self.record {
            SuspendedRecord::Resident {
                record,
                authorized: true,
                ..
            } => Some(record),
            _ => None,
        }
    }

    pub(super) fn authorized_resident_active_id(&self) -> Option<u64> {
        match self.record {
            SuspendedRecord::Resident {
                authorized: true,
                active_unit_id,
                ..
            } => Some(active_unit_id),
            _ => None,
        }
    }

    pub(super) fn replacement_record(&self) -> Result<CallbackRecord32, HostError> {
        match self.record {
            SuspendedRecord::Replacement(record) => Ok(record),
            SuspendedRecord::Resident { .. } => Err(call_error(CallError::InvalidToken)),
        }
    }

    pub(super) fn outer_token(&self) -> u32 {
        self.outer.token
    }

    pub(super) fn matches_return(&self, pc: u32, id: u32) -> bool {
        let (return_pc, return_id) = match self.record {
            SuspendedRecord::Replacement(record) => (record.return_pc, record.return_id),
            SuspendedRecord::Resident { record, .. } => (record.return_pc, record.return_id),
        };
        return_pc == pc && return_id == id
    }
}

impl EngineInstance {
    #[allow(clippy::too_many_arguments)]
    pub fn begin_callback_from_transfer(
        &mut self,
        key: u64,
        generation: u32,
        outer_token: u32,
        entry_pc: u32,
        return_pc: u32,
        return_id: u32,
        count: u32,
    ) -> Result<CallbackRecord32, HostError> {
        self.callback_request(key, generation, count as usize)?;
        let mut arguments = [0; MAX_STACK_WORDS];
        for (index, argument) in arguments[..count as usize].iter_mut().enumerate() {
            let offset = TRANSFER_OFFSET + index * 4;
            *argument = u32::from_le_bytes(self.arena()[offset..offset + 4].try_into().unwrap());
        }
        self.begin_callback(
            key,
            generation,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            &arguments[..count as usize],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_callback(
        &mut self,
        key: u64,
        generation: u32,
        outer_token: u32,
        entry_pc: u32,
        return_pc: u32,
        return_id: u32,
        arguments: &[u32],
    ) -> Result<CallbackRecord32, HostError> {
        self.callback_request(key, generation, arguments.len())?;
        let outer = self
            .pending_call
            .as_ref()
            .ok_or(call_error(CallError::InvalidToken))?;
        if outer_token == 0
            || outer.token != outer_token
            || outer.owner != PendingOwner::Replacement(generation)
        {
            return Err(call_error(CallError::InvalidToken));
        }
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != outer.state
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != outer.exit
            || self.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE] != outer.x87
        {
            return Err(call_error(CallError::StateChanged));
        }
        if self.call_cancelled() {
            return Err(call_error(CallError::Cancelled));
        }
        let artifact = self.artifact.as_ref().unwrap();
        let outer_exit =
            decode_exit(&outer.exit).map_err(|_| call_error(CallError::InvalidStop))?;
        let ExitReason::Gate { id: outer_id } = outer_exit.reason else {
            return Err(call_error(CallError::InvalidStop));
        };
        if !artifact.contains_instruction(entry_pc)
            || !artifact.matches_gate(return_pc, return_id)
            || return_id == outer_id
        {
            return Err(call_error(CallError::InvalidRequest));
        }
        let token = self
            .call_token
            .checked_add(1)
            .ok_or(call_error(CallError::TokenExhausted))?;
        let frame = CallbackFrame32::prepare(*outer.frame.state(), entry_pc, return_pc, arguments)
            .map_err(|error| match error {
                FrameError::InvalidRequest => call_error(CallError::InvalidRequest),
                FrameError::Memory(error) => call_error(CallError::Memory(error)),
            })?;
        let mut record = CallbackRecord32 {
            token,
            outer_token,
            phase: 1,
            outcome: 0,
            entry_pc,
            entry_esp: frame.state().registers[4],
            return_pc,
            return_id,
            stack_words: arguments.len() as u32,
            result: 0,
            generation,
        };
        let mut state = [0; STATE_SIZE];
        let mut exit = [0; EXIT_SIZE];
        let mut output = [0; CALLBACK_RECORD_SIZE];
        encode_state(frame.state(), &mut state).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit,
        )
        .map_err(|_| HostError::Infrastructure)?;
        encode_callback(&record, &mut output).map_err(|_| HostError::Infrastructure)?;

        self.memory
            .as_mut()
            .unwrap()
            .write_words32(frame.writes())
            .map_err(|error| call_error(CallError::Memory(error)))?;
        // after RAM publication only infallible record/authority publication remains.
        if self.artifact_bytes().is_err() {
            record.outcome = 1;
            output[28..32].copy_from_slice(&1_u32.to_le_bytes());
        }
        let outer = self.pending_call.take().unwrap();
        self.callback = Some(SuspendedCallback {
            record: SuspendedRecord::Replacement(record),
            outer,
        });
        self.call_token = token;
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&exit);
        arena[TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE].copy_from_slice(&output);
        Ok(record)
    }

    pub fn finish_callback(
        &mut self,
        key: u64,
        generation: u32,
        token: u32,
    ) -> Result<CallbackRecord32, HostError> {
        self.guard_artifact(key, generation)?;
        let callback = self
            .callback
            .as_ref()
            .ok_or(call_error(CallError::InvalidToken))?;
        let record = callback.replacement_record()?;
        if token == 0 || record.token != token || record.generation != generation {
            return Err(call_error(CallError::InvalidToken));
        }
        if self.pending_call.is_some() {
            return Err(call_error(CallError::Busy));
        }
        decode_x87(&self.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let state = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| call_error(CallError::InvalidStop))?;
        let exit = decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| call_error(CallError::InvalidStop))?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(call_error(CallError::InvalidStop));
        };
        if !callback.matches_return(state.eip, id)
            || !self.artifact.as_ref().unwrap().matches_gate(state.eip, id)
            || state.registers[4] != callback.outer.frame.state().registers[4]
        {
            return Err(call_error(CallError::InvalidStop));
        }
        if self.call_cancelled() {
            return Err(call_error(CallError::Cancelled));
        }
        let record = CallbackRecord32 {
            phase: 2,
            outcome: 0,
            result: state.registers[0],
            ..record
        };
        let mut output = [0; CALLBACK_RECORD_SIZE];
        encode_callback(&record, &mut output).map_err(|_| HostError::Infrastructure)?;
        let callback = self.callback.take().unwrap();
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&callback.outer.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&callback.outer.exit);
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&callback.outer.x87);
        arena[TRANSFER_OFFSET..TRANSFER_OFFSET + CALLBACK_RECORD_SIZE].copy_from_slice(&output);
        self.pending_call = Some(callback.outer);
        Ok(record)
    }

    pub fn abort_callback(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if token == 0
            || self
                .callback
                .as_ref()
                .is_none_or(|callback| callback.token() != token)
        {
            return Err(call_error(CallError::InvalidToken));
        }
        let callback = self.callback.take().unwrap();
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&callback.outer.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&callback.outer.exit);
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&callback.outer.x87);
        self.pending_call = Some(callback.outer);
        Ok(())
    }

    fn callback_request(&self, key: u64, generation: u32, count: usize) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        if self.callback.is_some() {
            return Err(call_error(CallError::Busy));
        }
        if count > MAX_STACK_WORDS {
            return Err(call_error(CallError::InvalidRequest));
        }
        Ok(())
    }
}

fn call_error(error: CallError) -> HostError {
    HostError::Call(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cpu::x86::State32, windows::CallingConvention32};

    #[test]
    fn last_callback_token_is_committed_once_and_exhaustion_precedes_stack_writes() {
        let mut engine = EngineInstance::new(2, 91).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        for (address, bytes) in [
            (0x1000, &[0x0f, 0x0b][..]),
            (0x1100, &[0x90][..]),
            (0x1200, &[0x0f, 0x0b][..]),
        ] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                .copy_from_slice(bytes);
            engine.upload(address, bytes.len() as u32).unwrap();
        }
        for (index, value) in [0x1000_u32, 2, 0x1100, 1, 0x1200, 2, 0x1000, 17, 0x1200, 18]
            .into_iter()
            .enumerate()
        {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let generation = engine.compile_with_gates(3, 2).unwrap();
        engine.write32(0x8010, 0x2000).unwrap();
        let mut state = State32 {
            eip: 0x1000,
            ..State32::default()
        };
        state.registers[4] = 0x8010;
        encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 1,
                reason: ExitReason::Gate { id: 17 },
            },
            &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        let outer = engine
            .capture_call(91, generation, CallingConvention32::Cdecl, 0)
            .unwrap();
        assert_eq!(outer.token, 1);
        engine.call_token = u32::MAX - 1;
        let callback = engine
            .begin_callback(91, generation, outer.token, 0x1100, 0x1200, 18, &[])
            .unwrap();
        assert_eq!(callback.token, u32::MAX);
        assert_eq!(engine.call_token, u32::MAX);
        engine.unmap(0x8000, 1).unwrap();
        engine.abort_callback(91, callback.token).unwrap();
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.begin_callback(91, generation, outer.token, 0x1100, 0x1200, 18, &[]),
            Err(call_error(CallError::TokenExhausted))
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.call_token, u32::MAX);
        assert!(engine.callback.is_none());
        assert_eq!(engine.pending_call.as_ref().unwrap().token, outer.token);
        assert_eq!(
            engine.guard(91, generation),
            Err(call_error(CallError::Busy))
        );
        engine
            .complete_call(91, generation, outer.token, 55)
            .unwrap();
        assert_eq!(engine.call_token, u32::MAX);
    }
}
