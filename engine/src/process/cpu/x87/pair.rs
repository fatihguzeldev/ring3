use super::{step::X87Request, *};
use crate::{
    abi::{arena::CANCEL_OFFSET, header},
    cpu::{
        dbt::X87PairStep32,
        x86::{SegmentContext, X87State, decode::CodeFetchContext},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct X87PairOutcome {
    pub(in crate::process) completed: u32,
    pub(in crate::process) reported_step: u32,
    pub(in crate::process) result: Result<(), HostError>,
}

impl X87PairOutcome {
    fn failure(completed: u32, reported_step: u32, error: HostError) -> Self {
        Self {
            completed,
            reported_step,
            result: Err(error),
        }
    }

    fn complete(completed: u32, reported_step: u32) -> Self {
        Self {
            completed,
            reported_step,
            result: Ok(()),
        }
    }
}

#[derive(Clone, Copy)]
struct Packet {
    length: u32,
    value: u32,
    result: Result<(), CpuMemoryError>,
}

impl EngineInstance {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process) fn x87_resident_pair(
        &mut self,
        key: u64,
        id: u64,
        first_pc: u32,
        addresses: [u32; 2],
        flags: u32,
        max_steps: u32,
    ) -> X87PairOutcome {
        let prepare = (|| {
            let region = self.guard_material_resident_region(key, id)?;
            if !(1..=2).contains(&max_steps) {
                return Err(HostError::InvalidRequest);
            }
            let descriptors = region.x87_pair(first_pc).ok_or(HostError::InvalidRequest)?;
            let requests = [0, 1].map(|index| {
                let X87PairStep32 {
                    operation,
                    pc,
                    opcode,
                    control,
                    ..
                } = descriptors[index];
                X87Request::new(operation, addresses[index], pc, u32::from(opcode), control)
            });
            let requests = [requests[0]?, requests[1]?];
            let state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::Infrastructure)?;
            Ok((descriptors, requests, state))
        })();
        let (descriptors, requests, mut state) = match prepare {
            Ok(prepared) => prepared,
            Err(error) => return X87PairOutcome::failure(0, 0, error),
        };
        let mut context: Option<SegmentContext> = None;
        let mut completed = 0;
        let mut publish_state = false;
        let mut packet = None;
        for index in 0..max_steps as usize {
            if index == 1 {
                let permitted = context
                    .as_ref()
                    .and_then(CodeFetchContext::from_segments)
                    .is_some_and(|cs| {
                        cs.permits(GuestAddress(descriptors[1].pc), descriptors[1].length)
                    });
                if header::read_u32(&self.arena, CANCEL_OFFSET) != 0
                    || !permitted
                    || requests[1].waiting && state.status & 0x3f & !state.control != 0
                {
                    return self.finish_x87_pair(
                        &state,
                        publish_state,
                        packet,
                        X87PairOutcome::complete(completed, 0),
                    );
                }
            }
            let outcome =
                match self.x87_step(&mut state, &mut context, requests[index], flags, &mut None) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        return self.finish_x87_pair(
                            &state,
                            publish_state,
                            packet,
                            X87PairOutcome::failure(completed, index as u32, error),
                        );
                    }
                };
            if outcome.memory_len != 0 {
                return self.finish_x87_pair(
                    &state,
                    publish_state,
                    packet,
                    X87PairOutcome::failure(completed, index as u32, HostError::Infrastructure),
                );
            }
            if outcome.publish_state {
                publish_state = true;
            }
            packet = Some(Packet {
                length: requests[index].length,
                value: outcome.value,
                result: outcome.result,
            });
            if outcome.result.is_err() {
                return self.finish_x87_pair(
                    &state,
                    publish_state,
                    packet,
                    X87PairOutcome::complete(completed, index as u32),
                );
            }
            completed += 1;
        }
        self.finish_x87_pair(
            &state,
            publish_state,
            packet,
            X87PairOutcome::complete(completed, completed - 1),
        )
    }

    fn finish_x87_pair(
        &mut self,
        state: &X87State,
        publish_state: bool,
        packet: Option<Packet>,
        mut outcome: X87PairOutcome,
    ) -> X87PairOutcome {
        if publish_state
            && encode_x87(state, &mut self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).is_err()
        {
            outcome.result = Err(HostError::Infrastructure);
            return outcome;
        }
        if let Some(packet) = packet
            && encode_cpu_result(
                packet.length,
                packet.value,
                packet.result,
                &mut self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
            )
            .is_err()
        {
            outcome.result = Err(HostError::Infrastructure);
        }
        outcome
    }
}

#[cfg(test)]
mod tests;
