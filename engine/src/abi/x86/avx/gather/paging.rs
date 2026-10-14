use super::*;
use crate::{
    cpu::x86::{PagingAccess32, PagingDomain32},
    memory::GuestAddress,
};

/// retained instruction inputs, not fields supplied by a helper reply.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GatherPagingGeometry {
    pub selected: u8,
    pub lanes: u8,
    pub width: u32,
    pub fault_start: GuestAddress,
}

/// structural wire facts only. decoding never creates a verified fault or records occurrence/cr2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherPagingReply {
    pub completed_lanes: u8,
    pub fault_lane: u8,
    linear: GuestAddress,
    error_code: u32,
}
impl GatherPagingReply {
    pub(crate) fn agrees_with(self, error: CpuPagingError32) -> bool {
        match error {
            CpuPagingError32::PageFault(fault) => {
                self.error_code == fault.error_code() && self.linear == fault.linear()
            }
            CpuPagingError32::Memory(_) => false,
        }
    }
}

pub(super) fn typed_fields(
    error: CpuPagingError32,
    width: u32,
) -> Result<(u32, u32, u32, u32), AbiError> {
    let valid = match error {
        CpuPagingError32::PageFault(fault) => {
            fault.access() == PagingAccess32::Read && fault.domain() == PagingDomain32::User
        }
        CpuPagingError32::Memory(_) => false,
    };
    if !valid {
        return Err(AbiError::MemoryHelper);
    }
    let fields = crate::abi::cpu_helper::paging::fields(width, error)?;
    Ok((fields[0], fields[2], fields[3], fields[4]))
}

pub(crate) fn decode_gather_paging_result(
    input: &[u8],
    geometry: GatherPagingGeometry,
) -> Result<GatherPagingReply, AbiError> {
    header::validate_versions(
        input,
        GATHER_RESULT_MAGIC,
        GATHER_RESULT_SIZE,
        &[GATHER_PAGING_RESULT_VERSION],
    )?;
    if !matches!(geometry.lanes, 2 | 4 | 8)
        || !matches!(geometry.width, 4 | 8)
        || geometry.selected == 0
        || u16::from(geometry.selected) >> geometry.lanes != 0
    {
        return Err(AbiError::MemoryHelper);
    }
    let completed = header::read_u32(input, GATHER_COMPLETED_OFFSET);
    let lane = header::read_u32(input, GATHER_FAULT_LANE_OFFSET);
    if lane >= u32::from(geometry.lanes)
        || geometry.selected & (1 << lane) == 0
        || completed != u32::from(geometry.selected) & ((1 << lane) - 1)
    {
        return Err(AbiError::MemoryHelper);
    }
    let linear = GuestAddress(header::read_u32(input, GATHER_FAULT_ADDRESS_OFFSET));
    if linear.0.wrapping_sub(geometry.fault_start.0) >= geometry.width {
        return Err(AbiError::MemoryHelper);
    }
    let detail = header::read_u32(input, GATHER_DETAIL_OFFSET);
    let access = header::read_u32(input, GATHER_ACCESS_OFFSET);
    if header::read_u32(input, GATHER_STATUS_OFFSET) != 9 || !matches!(detail, 4 | 5) || access != 1
    {
        return Err(AbiError::MemoryHelper);
    }
    Ok(GatherPagingReply {
        completed_lanes: completed as u8,
        fault_lane: lane as u8,
        linear,
        error_code: detail,
    })
}

#[cfg(test)]
mod tests;
