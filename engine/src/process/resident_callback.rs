#![forbid(unsafe_code)]

use super::{
    CallError, EngineInstance, HostError,
    call::PendingOwner,
    callback::{SuspendedCallback, SuspendedRecord},
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        resident_callback::{
            RESIDENT_CALLBACK_RECORD_SIZE, ResidentCallbackRecord32, encode_resident_callback,
        },
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason},
    windows::{CallbackFrame32, FrameError, MAX_STACK_WORDS},
};

impl EngineInstance {
    pub fn authorize_resident_callback(
        &mut self,
        key: u64,
        callback_id: u64,
        token: u32,
    ) -> Result<(), HostError> {
        let callback_unit = self.guard_resident_unit(key, callback_id)?;
        let callback = self
            .callback
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let SuspendedRecord::Resident { record, authorized } = callback.record else {
            return Err(HostError::Call(CallError::InvalidToken));
        };
        if token == 0 || record.token != token || record.callback_unit_id != callback_id {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        let outer_unit = self.guard_resident_unit(key, record.outer_unit_id)?;
        if authorized || self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if record.outer_unit_id == callback_id {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let mut initial = *callback.outer.frame.state();
        initial.eip = record.entry_pc;
        initial.registers[4] = record.entry_esp;
        let mut state = [0; STATE_SIZE];
        let mut exit = [0; EXIT_SIZE];
        encode_state(&initial, &mut state).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit,
        )
        .map_err(|_| HostError::Infrastructure)?;
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != state
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != exit
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let outer_exit = decode_exit(&callback.outer.exit)
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let ExitReason::Gate { id: outer_gate_id } = outer_exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        if !outer_unit.matches_gate(callback.outer.frame.state().eip, outer_gate_id) {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if !callback_unit.contains_instruction(record.entry_pc)
            || !callback_unit.matches_gate(record.return_pc, record.return_id)
            || record.return_id == outer_gate_id
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let SuspendedRecord::Resident { authorized, .. } =
            &mut self.callback.as_mut().unwrap().record
        else {
            unreachable!()
        };
        *authorized = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_resident_callback_from_transfer(
        &mut self,
        key: u64,
        outer_id: u64,
        callback_id: u64,
        outer_token: u32,
        entry_pc: u32,
        return_pc: u32,
        return_id: u32,
        count: u32,
    ) -> Result<ResidentCallbackRecord32, HostError> {
        self.resident_callback_request(key, outer_id, callback_id, count as usize)?;
        let mut arguments = [0; MAX_STACK_WORDS];
        for (index, argument) in arguments[..count as usize].iter_mut().enumerate() {
            let offset = TRANSFER_OFFSET + index * 4;
            *argument = u32::from_le_bytes(self.arena()[offset..offset + 4].try_into().unwrap());
        }
        self.begin_resident_callback(
            key,
            outer_id,
            callback_id,
            outer_token,
            entry_pc,
            return_pc,
            return_id,
            &arguments[..count as usize],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_resident_callback(
        &mut self,
        key: u64,
        outer_id: u64,
        callback_id: u64,
        outer_token: u32,
        entry_pc: u32,
        return_pc: u32,
        return_id: u32,
        arguments: &[u32],
    ) -> Result<ResidentCallbackRecord32, HostError> {
        self.resident_callback_request(key, outer_id, callback_id, arguments.len())?;
        let outer = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if outer_token == 0
            || outer.token != outer_token
            || outer.owner != PendingOwner::Resident(outer_id)
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != outer.state
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != outer.exit
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let outer_exit =
            decode_exit(&outer.exit).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let ExitReason::Gate { id: outer_gate_id } = outer_exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        if !self
            .guard_resident_unit(key, outer_id)?
            .matches_gate(outer.frame.state().eip, outer_gate_id)
        {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        let callback_unit = self.guard_resident_unit(key, callback_id)?;
        if !callback_unit.contains_instruction(entry_pc)
            || !callback_unit.matches_gate(return_pc, return_id)
            || return_id == outer_gate_id
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let token = self
            .call_token
            .checked_add(1)
            .ok_or(HostError::Call(CallError::TokenExhausted))?;
        let frame = CallbackFrame32::prepare(*outer.frame.state(), entry_pc, return_pc, arguments)
            .map_err(|error| match error {
                FrameError::InvalidRequest => HostError::Call(CallError::InvalidRequest),
                FrameError::Memory(error) => HostError::Call(CallError::Memory(error)),
            })?;
        let mut record = ResidentCallbackRecord32 {
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
            outer_unit_id: outer_id,
            callback_unit_id: callback_id,
        };
        let mut state = [0; STATE_SIZE];
        let mut exit = [0; EXIT_SIZE];
        let mut output = [0; RESIDENT_CALLBACK_RECORD_SIZE];
        encode_state(frame.state(), &mut state).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit,
        )
        .map_err(|_| HostError::Infrastructure)?;
        encode_resident_callback(&record, &mut output).map_err(|_| HostError::Infrastructure)?;
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(frame.writes())
            .map_err(|error| HostError::Call(CallError::Memory(error)))?;

        // committed stack writes may invalidate either retained unit; publication cannot fail.
        record.outcome = u32::from(self.resident_bytes(callback_id).is_err())
            | (u32::from(self.resident_bytes(outer_id).is_err()) << 1);
        output[28..32].copy_from_slice(&record.outcome.to_le_bytes());
        let outer = self.pending_call.take().unwrap();
        self.callback = Some(SuspendedCallback {
            record: SuspendedRecord::Resident {
                record,
                authorized: false,
            },
            outer,
        });
        self.call_token = token;
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&exit);
        arena[TRANSFER_OFFSET..TRANSFER_OFFSET + RESIDENT_CALLBACK_RECORD_SIZE]
            .copy_from_slice(&output);
        Ok(record)
    }

    fn resident_callback_request(
        &self,
        key: u64,
        outer_id: u64,
        callback_id: u64,
        count: usize,
    ) -> Result<(), HostError> {
        self.guard_resident_unit(key, outer_id)?;
        self.guard_resident_unit(key, callback_id)?;
        if self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if count > MAX_STACK_WORDS {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cpu::x86::State32, windows::CallingConvention32};

    #[test]
    fn last_resident_callback_token_commits_once_and_exhaustion_precedes_stack_access() {
        let mut engine = EngineInstance::new(2, 91).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        for (pc, bytes) in [
            (0x1000, &[0x0f, 0x0b][..]),
            (0x1100, &[0x90][..]),
            (0x1200, &[0x0f, 0x0b][..]),
        ] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                .copy_from_slice(bytes);
            engine.upload(pc, bytes.len() as u32).unwrap();
        }
        for (index, value) in [0x1000_u32, 2, 0x1100, 1, 0x1200, 2, 0x1000, 17, 0x1200, 18]
            .into_iter()
            .enumerate()
        {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
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
            .capture_resident_call(91, id, CallingConvention32::Cdecl, 0)
            .unwrap();
        assert_eq!(outer.token, 1);
        engine.call_token = u32::MAX - 1;
        let record = engine
            .begin_resident_callback(91, id, id, 1, 0x1100, 0x1200, 18, &[])
            .unwrap();
        assert_eq!(record.token, u32::MAX);
        assert_eq!(record.outcome, 0);
        assert_eq!(engine.call_token, u32::MAX);
        engine.unmap(0x8000, 1).unwrap();
        engine.abort_callback(91, u32::MAX).unwrap();
        let arena = engine.arena().to_vec();
        assert_eq!(
            engine.begin_resident_callback(91, id, id, 1, 0x1100, 0x1200, 18, &[]),
            Err(HostError::Call(CallError::TokenExhausted)),
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.call_token, u32::MAX);
        assert!(engine.callback.is_none());
        assert_eq!(
            engine.pending_call.as_ref().unwrap().owner,
            PendingOwner::Resident(id)
        );
        assert_eq!(engine.pending_call.as_ref().unwrap().token, 1);
        engine.complete_resident_call(91, id, 1, 55).unwrap();
        assert_eq!(engine.call_token, u32::MAX);
    }
}
