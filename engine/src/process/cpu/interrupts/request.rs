use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        exception::{EVENT_REQUEST_SIZE, EventRequest32, decode_event_request},
        x86::{
            EFLAGS_OFFSET, EIP_OFFSET, EXIT_SIZE, STATE_SIZE, decode_exit, decode_state,
            encode_exit_v6,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{EventDeliveryResult32, Register32, SegmentRegister},
    },
    process::{EngineInstance, HostError, StoreCompletion},
};

struct PreparedRequest {
    event: EventRequest32,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
}

#[cfg(test)]
mod tests;

impl EngineInstance {
    pub(in crate::process) fn deliver_exception_request(
        &mut self,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let prepared = self.prepare_event_request()?;
        let result = self.deliver_exception(prepared.event.origin, prepared.event.input)?;
        Ok(self.publish_event_entry(prepared, result))
    }

    pub(in crate::process) fn deliver_exception_resident_request(
        &mut self,
        key: u64,
        id: u64,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let prepared = self.prepare_event_request()?;
        let result =
            self.deliver_exception_resident(key, id, prepared.event.origin, prepared.event.input)?;
        Ok(self.publish_event_entry(prepared, result))
    }

    fn prepare_event_request(&self) -> Result<PreparedRequest, HostError> {
        let arena = self.arena();
        let event =
            decode_event_request(&arena[TRANSFER_OFFSET..TRANSFER_OFFSET + EVENT_REQUEST_SIZE])
                .map_err(|_| HostError::InvalidRequest)?;
        let state = decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::InvalidRequest)?;
        let context = self.operand_segment_context()?;
        if state.eip != event.input.eip
            || state.registers[Register32::Esp.index()] != event.input.esp
            || state.eflags != event.input.flags
            || context.state.selectors[SegmentRegister::Cs.index()] != event.input.cs
        {
            return Err(HostError::InvalidRequest);
        }
        let mut state_bytes = [0; STATE_SIZE];
        state_bytes.copy_from_slice(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]);
        state_bytes[4..6].copy_from_slice(&3_u16.to_le_bytes());
        state_bytes[12..16].fill(0);
        let mut exit_bytes = [0; EXIT_SIZE];
        encode_exit_v6(
            &ExecutionExit {
                retired: exit.retired,
                reason: ExitReason::NeedCode,
            },
            &mut exit_bytes,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let exit_version =
            u16::from_le_bytes([arena[EXIT_OFFSET + 4], arena[EXIT_OFFSET + 5]]).max(6);
        exit_bytes[4..6].copy_from_slice(&exit_version.to_le_bytes());
        Ok(PreparedRequest {
            event,
            state: state_bytes,
            exit: exit_bytes,
        })
    }

    fn publish_event_entry(
        &mut self,
        mut prepared: PreparedRequest,
        result: EventDeliveryResult32,
    ) -> StoreCompletion {
        let EventDeliveryResult32::Entered(entered) = result else {
            return StoreCompletion::Complete;
        };
        // every fallible operation precedes core frame publication; the remaining copies cannot fail.
        for (offset, value) in [
            (32, entered.esp),
            (EIP_OFFSET, entered.target),
            (EFLAGS_OFFSET, entered.flags),
        ] {
            prepared.state[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&prepared.state);
        arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE].copy_from_slice(&prepared.exit);
        if entered.code_invalidated {
            StoreCompletion::CodeInvalidated
        } else {
            StoreCompletion::Complete
        }
    }
}
