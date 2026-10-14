use crate::{
    abi::x86::{
        GATHER_RESULT_SIZE, GatherFaultKind, GatherFaultResult, GatherPagingGeometry, GatherResult,
        decode_gather_paging_result, encode_gather_paging_result, encode_gather_result,
    },
    cpu::x86::{
        CpuPagingError32, SegmentContext, SegmentRegister,
        avx::{GatherFaultCause, GatherInputs, GatherInstruction, GatherOutcome},
    },
    memory::GuestAddress,
    process::HostError,
};

pub(super) fn packet(
    instruction: GatherInstruction,
    inputs: GatherInputs,
    segment: SegmentRegister,
    base: u32,
    context: Option<&SegmentContext>,
    outcome: &GatherOutcome,
) -> Result<[u8; GATHER_RESULT_SIZE], HostError> {
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
                GatherFaultCause::Paging(error) => GatherFaultKind::Paging(error),
            },
        }),
    };
    let mut packet = [0; GATHER_RESULT_SIZE];
    let typed = result.fault.and_then(|fault| match fault.kind {
        GatherFaultKind::Paging(error) if !matches!(error, CpuPagingError32::Memory(_)) => {
            Some((fault.lane, error))
        }
        _ => None,
    });
    if let Some((lane, error)) = typed {
        let geometry = instruction.geometry();
        if usize::from(lane) >= geometry.lanes() {
            return Err(HostError::Infrastructure);
        }
        let context = context.ok_or(HostError::Infrastructure)?;
        let fault_start = GuestAddress(
            context.state.bases[segment.index()].wrapping_add(instruction.lane_offset(
                &inputs.index,
                usize::from(lane),
                base,
            )),
        );
        encode_gather_paging_result(result, geometry.data_bytes() as u32, &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        let parsed = decode_gather_paging_result(
            &packet,
            GatherPagingGeometry {
                selected: instruction.selected(&inputs.mask),
                lanes: geometry.lanes() as u8,
                width: geometry.data_bytes() as u32,
                fault_start,
            },
        )
        .map_err(|_| HostError::Infrastructure)?;
        if parsed.fault_lane != lane
            || parsed.completed_lanes != outcome.completed_lanes
            || !parsed.agrees_with(error)
        {
            return Err(HostError::Infrastructure);
        }
    } else {
        encode_gather_result(result, &mut packet).map_err(|_| HostError::Infrastructure)?;
    }
    Ok(packet)
}

#[cfg(test)]
mod tests;
