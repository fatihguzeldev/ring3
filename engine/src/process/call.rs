#![forbid(unsafe_code)]

use super::{EngineInstance, HostError};
use crate::{
    abi::{
        arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        call_frame::{CALL_FRAME_SIZE, CallRecord32, encode_call_frame},
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason},
    memory::MemoryError,
    windows::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallError {
    Busy,
    InvalidRequest,
    InvalidStop,
    InvalidToken,
    StateChanged,
    Cancelled,
    TokenExhausted,
    Memory(MemoryError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingOwner {
    Replacement(u32),
    Resident(u64),
}

pub(super) struct PendingCall {
    pub(super) token: u32,
    pub(super) owner: PendingOwner,
    pub(super) frame: CallFrame32,
    pub(super) state: [u8; STATE_SIZE],
    pub(super) exit: [u8; EXIT_SIZE],
}

impl EngineInstance {
    pub fn capture_resident_call(
        &mut self,
        key: u64,
        id: u64,
        convention: CallingConvention32,
        stack_words: u32,
    ) -> Result<CallRecord32, HostError> {
        let tag = match convention {
            CallingConvention32::Cdecl => 1,
            CallingConvention32::Stdcall => 2,
            CallingConvention32::Thiscall => 3,
        };
        self.capture_resident_call_raw(key, id, tag, stack_words)
    }

    pub fn capture_resident_call_raw(
        &mut self,
        key: u64,
        id: u64,
        convention_tag: u32,
        stack_words: u32,
    ) -> Result<CallRecord32, HostError> {
        self.guard_resident(key, id)?;
        self.capture_call_frame(PendingOwner::Resident(id), convention_tag, stack_words)
    }

    pub fn capture_call(
        &mut self,
        key: u64,
        generation: u32,
        convention: CallingConvention32,
        stack_words: u32,
    ) -> Result<CallRecord32, HostError> {
        let tag = match convention {
            CallingConvention32::Cdecl => 1,
            CallingConvention32::Stdcall => 2,
            CallingConvention32::Thiscall => 3,
        };
        self.capture_call_raw(key, generation, tag, stack_words)
    }

    pub fn capture_call_raw(
        &mut self,
        key: u64,
        generation: u32,
        convention_tag: u32,
        stack_words: u32,
    ) -> Result<CallRecord32, HostError> {
        self.guard(key, generation)?;
        self.capture_call_frame(
            PendingOwner::Replacement(generation),
            convention_tag,
            stack_words,
        )
    }

    fn capture_call_frame(
        &mut self,
        owner: PendingOwner,
        convention_tag: u32,
        stack_words: u32,
    ) -> Result<CallRecord32, HostError> {
        if stack_words > MAX_STACK_WORDS as u32 {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let convention = CallingConvention32::try_from(convention_tag)
            .map_err(|_| HostError::Call(CallError::InvalidRequest))?;
        let state_bytes: [u8; STATE_SIZE] = self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
            .try_into()
            .unwrap();
        let exit_bytes: [u8; EXIT_SIZE] = self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap();
        let state =
            decode_state(&state_bytes).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let exit = decode_exit(&exit_bytes).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let ExitReason::Gate { id } = exit.reason else {
            return Err(HostError::Call(CallError::InvalidStop));
        };
        let matches_gate = match owner {
            PendingOwner::Replacement(_) => self
                .artifact
                .as_ref()
                .is_some_and(|artifact| artifact.matches_gate(state.eip, id)),
            PendingOwner::Resident(unit) => self
                .guard_resident_unit(self.key, unit)?
                .matches_gate(state.eip, id),
        };
        if !matches_gate {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if self
            .callback
            .as_ref()
            .is_some_and(|callback| callback.matches_return(state.eip, id))
        {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let token = self
            .call_token
            .checked_add(1)
            .ok_or(HostError::Call(CallError::TokenExhausted))?;
        let frame = CallFrame32::capture(self.memory()?, state, convention, stack_words).map_err(
            |error| {
                HostError::Call(match error {
                    FrameError::InvalidRequest => CallError::InvalidRequest,
                    FrameError::Memory(error) => CallError::Memory(error),
                })
            },
        )?;
        let record = CallRecord32 {
            token,
            id,
            convention: convention_tag,
            stack_words,
            gate_pc: state.eip,
            entry_esp: state.registers[4],
            return_pc: frame.return_pc(),
            this_pointer: frame.this_pointer().unwrap_or(0),
            arguments: std::array::from_fn(|index| {
                frame.arguments().get(index).copied().unwrap_or(0)
            }),
        };
        let mut output = [0; CALL_FRAME_SIZE];
        encode_call_frame(&record, &mut output).map_err(|_| HostError::Infrastructure)?;
        self.pending_call = Some(PendingCall {
            token,
            owner,
            frame,
            state: state_bytes,
            exit: exit_bytes,
        });
        self.call_token = token;
        self.arena.as_mut().get_mut()[TRANSFER_OFFSET..TRANSFER_OFFSET + CALL_FRAME_SIZE]
            .copy_from_slice(&output);
        Ok(record)
    }

    pub fn complete_call(
        &mut self,
        key: u64,
        generation: u32,
        token: u32,
        result: u32,
    ) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        self.complete_call_frame(PendingOwner::Replacement(generation), token, result)
    }

    pub fn complete_resident_call(
        &mut self,
        key: u64,
        id: u64,
        token: u32,
        result: u32,
    ) -> Result<(), HostError> {
        self.guard_resident_unit(key, id)?;
        self.complete_call_frame(PendingOwner::Resident(id), token, result)
    }

    fn complete_call_frame(
        &mut self,
        owner: PendingOwner,
        token: u32,
        result: u32,
    ) -> Result<(), HostError> {
        if self
            .callback
            .as_ref()
            .is_some_and(|callback| callback.outer_token() == token)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        let pending = self
            .pending_call
            .as_ref()
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        if token == 0 || pending.token != token || pending.owner != owner {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] != pending.state
            || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != pending.exit
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if self.call_cancelled() {
            return Err(HostError::Call(CallError::Cancelled));
        }
        let state = pending.frame.complete(result);
        let mut state_bytes = [0; STATE_SIZE];
        let mut exit_bytes = [0; EXIT_SIZE];
        encode_state(&state, &mut state_bytes).map_err(|_| HostError::Infrastructure)?;
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut exit_bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&state_bytes);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&exit_bytes);
        self.pending_call = None;
        Ok(())
    }

    pub fn abandon_call(&mut self, key: u64, token: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self
            .callback
            .as_ref()
            .is_some_and(|callback| callback.outer_token() == token)
        {
            return Err(HostError::Call(CallError::Busy));
        }
        if token == 0
            || self
                .pending_call
                .as_ref()
                .is_none_or(|pending| pending.token != token)
        {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.pending_call = None;
        Ok(())
    }

    pub(super) fn call_cancelled(&self) -> bool {
        u32::from_le_bytes(
            self.arena()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
                .try_into()
                .unwrap(),
        ) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::x86::State32;

    #[test]
    fn last_call_token_is_issued_once_and_exhaustion_precedes_guest_reads() {
        let mut instance = EngineInstance::new(2, 91).unwrap();
        instance.map(0x1000, 1, 7).unwrap();
        instance.map(0x8000, 1, 3).unwrap();
        instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        instance.upload(0x1000, 2).unwrap();
        for (index, value) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
            instance.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let generation = instance.compile_with_gates(1, 1).unwrap();
        instance.write32(0x8000, 0x1234_5678).unwrap();
        let mut state = State32 {
            eip: 0x1000,
            ..State32::default()
        };
        state.registers[4] = 0x8000;
        encode_state(&state, &mut instance.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 1,
                reason: ExitReason::Gate { id: 77 },
            },
            &mut instance.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
        )
        .unwrap();
        instance.call_token = u32::MAX - 1;
        let record = instance
            .capture_call(91, generation, CallingConvention32::Cdecl, 0)
            .unwrap();
        assert_eq!(record.token, u32::MAX);
        instance.abandon_call(91, record.token).unwrap();
        instance.unmap(0x8000, 1).unwrap();
        let arena = instance.arena().to_vec();
        assert_eq!(
            instance.capture_call(91, generation, CallingConvention32::Cdecl, 0),
            Err(HostError::Call(CallError::TokenExhausted)),
        );
        assert_eq!(instance.arena(), arena);
        assert_eq!(instance.call_token, u32::MAX);
        assert!(instance.pending_call.is_none());
        assert_eq!(instance.guard(91, generation), Ok(()));
    }
}
