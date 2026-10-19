use super::{
    group::{Packet, X87GroupOutcome},
    step::X87Request,
    *,
};
use crate::{
    abi::{arena::CANCEL_OFFSET, header},
    cpu::{
        dbt::X87_WIDE_GROUP_MAX_STEPS,
        x86::{SegmentContext, decode::CodeFetchContext},
    },
};

#[cfg(test)]
mod tests;

impl EngineInstance {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process) fn x87_resident_group32(
        &mut self,
        key: u64,
        id: u64,
        ordinal: u32,
        addresses: [u32; X87_WIDE_GROUP_MAX_STEPS],
        flags: u32,
        max_steps: u32,
    ) -> X87GroupOutcome {
        let prepare = (|| {
            let region = self.guard_material_resident_region(key, id)?;
            let group = region
                .native_x87_group32(ordinal)
                .ok_or(HostError::InvalidRequest)?;
            let length = group.steps().len();
            if max_steps == 0 || max_steps as usize > length {
                return Err(HostError::InvalidRequest);
            }
            let state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::Infrastructure)?;
            Ok((group, state))
        })();
        let (group, mut state) = match prepare {
            Ok(prepared) => prepared,
            Err(error) => return X87GroupOutcome::failure(0, 0, error),
        };
        let mut context: Option<SegmentContext> = None;
        let mut completed = 0;
        let mut publish_state = false;
        let mut packet = None;
        for index in 0..max_steps as usize {
            let descriptor = &group.steps()[index];
            let request = X87Request::prepared(descriptor, addresses[index]);
            if index != 0 {
                let permitted = context
                    .as_ref()
                    .and_then(CodeFetchContext::from_segments)
                    .is_some_and(|cs| {
                        cs.permits(GuestAddress(request.pc), descriptor.instruction_length)
                    });
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
                if index + 1 != group.steps().len() {
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
}
