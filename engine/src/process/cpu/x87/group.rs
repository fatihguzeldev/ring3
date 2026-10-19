use super::{step::X87Request, *};
use crate::{
    abi::{arena::CANCEL_OFFSET, header},
    cpu::{
        dbt::{X87_GROUP_MAX_STEPS, X87Group32},
        x86::{SegmentContext, X87State, decode::CodeFetchContext},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct X87GroupOutcome {
    pub(in crate::process) completed: u32,
    pub(in crate::process) reported_step: u32,
    pub(in crate::process) result: Result<StoreCompletion, HostError>,
}

#[cfg(test)]
mod tests;

impl X87GroupOutcome {
    pub(super) fn failure(completed: u32, reported_step: u32, error: HostError) -> Self {
        Self {
            completed,
            reported_step,
            result: Err(error),
        }
    }

    pub(super) fn complete(completed: u32, reported_step: u32) -> Self {
        Self {
            completed,
            reported_step,
            result: Ok(StoreCompletion::Complete),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Packet {
    pub(super) length: u32,
    pub(super) value: u32,
    pub(super) result: Result<(), CpuMemoryError>,
}

fn requests(
    group: &X87Group32,
    addresses: [u32; X87_GROUP_MAX_STEPS],
) -> Result<[X87Request; X87_GROUP_MAX_STEPS], HostError> {
    let first = group.steps()[0];
    let first = X87Request::new(
        first.operation,
        addresses[0],
        first.pc,
        u32::from(first.opcode),
        first.control,
    )?;
    let mut requests = [first; X87_GROUP_MAX_STEPS];
    for (index, descriptor) in group.steps().iter().enumerate().skip(1) {
        requests[index] = X87Request::new(
            descriptor.operation,
            addresses[index],
            descriptor.pc,
            u32::from(descriptor.opcode),
            descriptor.control,
        )?;
    }
    Ok(requests)
}

impl EngineInstance {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process) fn x87_resident_group(
        &mut self,
        key: u64,
        id: u64,
        ordinal: u32,
        addresses: [u32; X87_GROUP_MAX_STEPS],
        flags: u32,
        max_steps: u32,
    ) -> X87GroupOutcome {
        let prepare = (|| {
            let region = self.guard_material_resident_region(key, id)?;
            let group = region.x87_group(ordinal).ok_or(HostError::InvalidRequest)?;
            if max_steps == 0 || max_steps as usize > group.len() {
                return Err(HostError::InvalidRequest);
            }
            let requests = requests(&group, addresses)?;
            let state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::Infrastructure)?;
            Ok((group, requests, state))
        })();
        let (group, requests, mut state) = match prepare {
            Ok(prepared) => prepared,
            Err(error) => return X87GroupOutcome::failure(0, 0, error),
        };
        let mut context: Option<SegmentContext> = None;
        let mut completed = 0;
        let mut publish_state = false;
        let mut packet = None;
        for index in 0..max_steps as usize {
            let request = requests[index];
            if index != 0 {
                let descriptor = group.steps()[index];
                let permitted = context
                    .as_ref()
                    .and_then(CodeFetchContext::from_segments)
                    .is_some_and(|cs| cs.permits(GuestAddress(descriptor.pc), descriptor.length));
                if header::read_u32(&self.arena, CANCEL_OFFSET) != 0
                    || !permitted
                    || request.waiting && state.status & 0x3f & !state.control != 0
                {
                    return self.finish_x87_group(
                        &state,
                        publish_state,
                        packet,
                        X87GroupOutcome::complete(completed, completed - 1),
                    );
                }
            }
            if request.write_size != 0 {
                if index + 1 != group.len() {
                    return self.finish_x87_group(
                        &state,
                        publish_state,
                        packet,
                        X87GroupOutcome::failure(
                            completed,
                            index as u32,
                            HostError::Infrastructure,
                        ),
                    );
                }
                let prefix = self.finish_x87_group(
                    &state,
                    publish_state,
                    packet,
                    X87GroupOutcome::complete(completed, completed - 1),
                );
                if prefix.result.is_err() {
                    return prefix;
                }
                return self.x87_final_store_group(
                    id,
                    &mut state,
                    &mut context,
                    request,
                    flags,
                    completed,
                );
            }
            let outcome = match self.x87_step(&mut state, &mut context, request, flags, &mut None) {
                Ok(outcome) => outcome,
                Err(error) => {
                    return self.finish_x87_group(
                        &state,
                        publish_state,
                        packet,
                        X87GroupOutcome::failure(completed, index as u32, error),
                    );
                }
            };
            if outcome.memory_len != 0 {
                return self.finish_x87_group(
                    &state,
                    publish_state,
                    packet,
                    X87GroupOutcome::failure(completed, index as u32, HostError::Infrastructure),
                );
            }
            publish_state |= outcome.publish_state;
            packet = Some(Packet {
                length: request.length,
                value: outcome.value,
                result: outcome.result,
            });
            if outcome.result.is_err() {
                return self.finish_x87_group(
                    &state,
                    publish_state,
                    packet,
                    X87GroupOutcome::complete(completed, index as u32),
                );
            }
            completed += 1;
        }
        self.finish_x87_group(
            &state,
            publish_state,
            packet,
            X87GroupOutcome::complete(completed, completed - 1),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn x87_final_store_group(
        &mut self,
        id: u64,
        state: &mut X87State,
        context: &mut Option<SegmentContext>,
        request: X87Request,
        flags: u32,
        completed: u32,
    ) -> X87GroupOutcome {
        let mut output = None;
        let outcome = match self.x87_step(state, context, request, flags, &mut output) {
            Ok(outcome) => outcome,
            Err(error) => return X87GroupOutcome::failure(completed, completed, error),
        };
        if outcome.result.is_err() || outcome.memory_len == 0 {
            return self.finish_x87_group(
                state,
                outcome.publish_state,
                Some(Packet {
                    length: request.length,
                    value: outcome.value,
                    result: outcome.result,
                }),
                X87GroupOutcome::complete(completed + u32::from(outcome.result.is_ok()), completed),
            );
        }
        match self.x87_commit_store(
            request,
            state,
            outcome.value,
            &output.as_ref().unwrap()[..outcome.memory_len],
            Some(id),
        ) {
            Ok(commit) => X87GroupOutcome {
                completed: completed + u32::from(commit.instruction_completed),
                reported_step: completed,
                result: Ok(commit.completion),
            },
            Err(error) => X87GroupOutcome::failure(completed, completed, error),
        }
    }

    pub(super) fn finish_x87_group(
        &mut self,
        state: &X87State,
        publish_state: bool,
        packet: Option<Packet>,
        mut outcome: X87GroupOutcome,
    ) -> X87GroupOutcome {
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
