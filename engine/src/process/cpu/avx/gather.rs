use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        x86::{
            GATHER_ADDRESS_SIZE, GATHER_ADDRESS_TRANSFER, GATHER_DESTINATION_OFFSET,
            GATHER_MASK_OFFSET, GATHER_OUTPUT_SIZE, GATHER_OUTPUT_TRANSFER, GATHER_RESULT_SIZE,
            GatherFaultKind, GatherFaultResult, GatherResult, decode_gather_address,
            encode_gather_result,
        },
    },
    cpu::x86::{
        SegmentRegister,
        avx::{GatherFaultCause, GatherInstruction, VectorState},
    },
    process::{EngineInstance, HostError, StoreCompletion},
};

impl EngineInstance {
    // the parent validates captured authority, the descriptor and canonical
    // vectors. gather returns staged images; guest publication belongs to DBT.
    pub(super) fn avx_gather_apply(
        &mut self,
        instruction: GatherInstruction,
        vectors: &VectorState,
    ) -> Result<StoreCompletion, HostError> {
        let start = TRANSFER_OFFSET + GATHER_ADDRESS_TRANSFER;
        let request = decode_gather_address(
            &self.arena.as_ref().get_ref()[start..start + GATHER_ADDRESS_SIZE],
        )
        .map_err(|_| HostError::InvalidRequest)?;
        let inputs = instruction.capture(vectors);
        let selected = instruction.selected(&inputs.mask);
        let segments = if selected == 0 {
            None
        } else {
            Some(self.segment_context()?)
        };
        let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
        let outcome = instruction
            .evaluate(
                inputs,
                request.segment,
                request.base_offset,
                segments.as_ref(),
                memory,
            )
            .map_err(|_| HostError::Infrastructure)?;
        let result = GatherResult {
            completed_lanes: outcome.completed_lanes,
            fault: outcome.fault.map(|fault| GatherFaultResult {
                lane: fault.lane,
                kind: match fault.cause {
                    GatherFaultCause::Segment {
                        segment: SegmentRegister::Ss,
                        ..
                    } => GatherFaultKind::StackSegment,
                    GatherFaultCause::Segment { .. } => GatherFaultKind::GeneralProtection,
                    GatherFaultCause::Memory(fault) => GatherFaultKind::Memory(fault),
                },
            }),
        };
        let mut output = [0; GATHER_OUTPUT_SIZE];
        output[GATHER_DESTINATION_OFFSET..GATHER_MASK_OFFSET].copy_from_slice(&outcome.destination);
        output[GATHER_MASK_OFFSET..].copy_from_slice(&outcome.mask);
        let mut packet = [0; GATHER_RESULT_SIZE];
        encode_gather_result(result, &mut packet).map_err(|_| HostError::Infrastructure)?;

        // finish every fallible step before publishing either scratch record.
        let arena = self.arena.as_mut().get_mut();
        let output_start = TRANSFER_OFFSET + GATHER_OUTPUT_TRANSFER;
        arena[output_start..output_start + GATHER_OUTPUT_SIZE].copy_from_slice(&output);
        arena[HELPER_OFFSET..HELPER_OFFSET + GATHER_RESULT_SIZE].copy_from_slice(&packet);
        Ok(StoreCompletion::Complete)
    }
}

#[cfg(test)]
#[path = "gather/tests.rs"]
mod tests;
