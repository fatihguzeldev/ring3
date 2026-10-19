use super::{
    group::{Packet, X87GroupOutcome},
    step::X87Request,
    *,
};
use crate::{
    abi::{arena::CANCEL_OFFSET, header},
    cpu::{
        dbt::{X87_WIDE_GROUP_MAX_STEPS, X87WideGroup32},
        x86::{SegmentContext, decode::CodeFetchContext},
    },
};

#[cfg(test)]
mod tests;

fn requests(
    group: X87WideGroup32<'_>,
    addresses: [u32; X87_WIDE_GROUP_MAX_STEPS],
) -> Result<
    (
        [X87Request; X87_WIDE_GROUP_MAX_STEPS],
        [u8; X87_WIDE_GROUP_MAX_STEPS],
    ),
    HostError,
> {
    let first = group.descriptor(0).ok_or(HostError::InvalidRequest)?;
    let first_request = X87Request::new(
        first.operation,
        addresses[0],
        first.pc,
        u32::from(first.opcode),
        first.control,
    )?;
    let mut requests = [first_request; X87_WIDE_GROUP_MAX_STEPS];
    let mut lengths = [0; X87_WIDE_GROUP_MAX_STEPS];
    lengths[0] = first.length;
    for index in 1..group.len() {
        let descriptor = group.descriptor(index).ok_or(HostError::InvalidRequest)?;
        requests[index] = X87Request::new(
            descriptor.operation,
            addresses[index],
            descriptor.pc,
            u32::from(descriptor.opcode),
            descriptor.control,
        )?;
        lengths[index] = descriptor.length;
    }
    Ok((requests, lengths))
}

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
                .x87_group32(ordinal)
                .ok_or(HostError::InvalidRequest)?;
            let length = group.len();
            if max_steps == 0 || max_steps as usize > length {
                return Err(HostError::InvalidRequest);
            }
            // only validated requests and instruction lengths leave the final-ir borrow.
            let (requests, lengths) = requests(group, addresses)?;
            let state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::Infrastructure)?;
            Ok((length, requests, lengths, state))
        })();
        let (length, requests, lengths, mut state) = match prepare {
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
                let permitted = context
                    .as_ref()
                    .and_then(CodeFetchContext::from_segments)
                    .is_some_and(|cs| cs.permits(GuestAddress(request.pc), lengths[index]));
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
                if index + 1 != length {
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
