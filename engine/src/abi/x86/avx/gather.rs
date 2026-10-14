use crate::{
    abi::{AbiError, header},
    cpu::x86::{CpuPagingError32, SegmentRegister},
    memory::{Access, FaultReason, MemoryFault},
};

pub(crate) const GATHER_ADDRESS_MAGIC: [u8; 4] = *b"R3GA";
pub(crate) const GATHER_ADDRESS_VERSION: u16 = 1;
pub(crate) const GATHER_ADDRESS_SIZE: usize = 24;
pub(crate) const GATHER_ADDRESS_TRANSFER: usize = 544;
pub(crate) const GATHER_ADDRESS_SEGMENT_OFFSET: usize = 16;
pub(crate) const GATHER_ADDRESS_BASE_OFFSET: usize = 20;

pub(crate) const GATHER_RESULT_MAGIC: [u8; 4] = *b"R3GH";
pub(crate) const GATHER_RESULT_VERSION: u16 = 1;
pub(crate) const GATHER_PAGING_RESULT_VERSION: u16 = 2;

mod paging;
pub(crate) use paging::{GatherPagingGeometry, decode_gather_paging_result};
pub(crate) const GATHER_RESULT_SIZE: usize = 40;
pub(crate) const GATHER_STATUS_OFFSET: usize = 16;
pub(crate) const GATHER_DETAIL_OFFSET: usize = 20;
pub(crate) const GATHER_FAULT_ADDRESS_OFFSET: usize = 24;
pub(crate) const GATHER_ACCESS_OFFSET: usize = 28;
pub(crate) const GATHER_COMPLETED_OFFSET: usize = 32;
pub(crate) const GATHER_FAULT_LANE_OFFSET: usize = 36;
pub(crate) const GATHER_NO_FAULT: u32 = u32::MAX;

pub(crate) const GATHER_OUTPUT_TRANSFER: usize = 0;
pub(crate) const GATHER_OUTPUT_SIZE: usize = 64;
pub(crate) const GATHER_DESTINATION_OFFSET: usize = 0;
pub(crate) const GATHER_MASK_OFFSET: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherRequest {
    pub segment: SegmentRegister,
    pub base_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatherFaultKind {
    Memory(MemoryFault),
    Paging(CpuPagingError32),
    GeneralProtection,
    StackSegment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherFaultResult {
    pub lane: u8,
    pub kind: GatherFaultKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatherResult {
    pub completed_lanes: u8,
    pub fault: Option<GatherFaultResult>,
}

pub(crate) fn decode_gather_address(input: &[u8]) -> Result<GatherRequest, AbiError> {
    header::validate_versions(
        input,
        GATHER_ADDRESS_MAGIC,
        GATHER_ADDRESS_SIZE,
        &[GATHER_ADDRESS_VERSION],
    )?;
    let segment = match header::read_u32(input, GATHER_ADDRESS_SEGMENT_OFFSET) {
        0 => SegmentRegister::Es,
        1 => SegmentRegister::Cs,
        2 => SegmentRegister::Ss,
        3 => SegmentRegister::Ds,
        4 => SegmentRegister::Fs,
        5 => SegmentRegister::Gs,
        _ => return Err(AbiError::Reserved),
    };
    Ok(GatherRequest {
        segment,
        base_offset: header::read_u32(input, GATHER_ADDRESS_BASE_OFFSET),
    })
}

// geometry, selected-prefix and fault-span checks belong to the instruction's
// contextual consumer. this codec closes the wire fields before writing any byte.
pub(crate) fn encode_gather_result(
    result: GatherResult,
    output: &mut [u8],
) -> Result<(), AbiError> {
    encode_result(result, None, output)
}

pub(crate) fn encode_gather_paging_result(
    result: GatherResult,
    lane_width: u32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if !matches!(lane_width, 4 | 8) {
        return Err(AbiError::MemoryHelper);
    }
    encode_result(result, Some(lane_width), output)
}

fn encode_result(
    result: GatherResult,
    lane_width: Option<u32>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != GATHER_RESULT_SIZE {
        return Err(AbiError::Length);
    }
    let mut version = GATHER_RESULT_VERSION;
    let (status, detail, address, access, lane) = match result.fault {
        None => (0, 0, 0, 0, GATHER_NO_FAULT),
        Some(GatherFaultResult { lane, kind }) => {
            if lane >= 8 || result.completed_lanes & !((1u8 << lane) - 1) != 0 {
                return Err(AbiError::MemoryHelper);
            }
            let fields = match kind {
                GatherFaultKind::Memory(fault)
                | GatherFaultKind::Paging(CpuPagingError32::Memory(
                    crate::memory::MemoryError::Fault(fault),
                )) => {
                    if fault.access != Access::Read {
                        return Err(AbiError::MemoryHelper);
                    }
                    (
                        1,
                        match fault.reason {
                            FaultReason::Unmapped => 1,
                            FaultReason::Permission => 2,
                            FaultReason::AddressOverflow => 3,
                        },
                        fault.address.0,
                        1,
                    )
                }
                GatherFaultKind::Paging(error) => {
                    let width = lane_width.ok_or(AbiError::MemoryHelper)?;
                    let fields = paging::typed_fields(error, width)?;
                    version = GATHER_PAGING_RESULT_VERSION;
                    fields
                }
                GatherFaultKind::GeneralProtection => (3, 0, 0, 0),
                GatherFaultKind::StackSegment => (7, 0, 0, 0),
            };
            (fields.0, fields.1, fields.2, fields.3, u32::from(lane))
        }
    };
    header::write_version(output, GATHER_RESULT_MAGIC, version);
    for (offset, value) in [
        (GATHER_STATUS_OFFSET, status),
        (GATHER_DETAIL_OFFSET, detail),
        (GATHER_FAULT_ADDRESS_OFFSET, address),
        (GATHER_ACCESS_OFFSET, access),
        (GATHER_COMPLETED_OFFSET, u32::from(result.completed_lanes)),
        (GATHER_FAULT_LANE_OFFSET, lane),
    ] {
        header::write_u32(output, offset, value);
    }
    Ok(())
}

#[cfg(test)]
#[path = "gather/tests.rs"]
mod tests;
