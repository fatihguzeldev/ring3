use crate::{
    abi::{AbiError, header},
    cpu::x86::SegmentRegister,
};

mod gather;
pub(crate) use gather::{
    GATHER_ACCESS_OFFSET, GATHER_ADDRESS_BASE_OFFSET, GATHER_ADDRESS_MAGIC,
    GATHER_ADDRESS_SEGMENT_OFFSET, GATHER_ADDRESS_SIZE, GATHER_ADDRESS_TRANSFER,
    GATHER_ADDRESS_VERSION, GATHER_COMPLETED_OFFSET, GATHER_DESTINATION_OFFSET,
    GATHER_DETAIL_OFFSET, GATHER_FAULT_ADDRESS_OFFSET, GATHER_FAULT_LANE_OFFSET,
    GATHER_MASK_OFFSET, GATHER_NO_FAULT, GATHER_OUTPUT_SIZE, GATHER_OUTPUT_TRANSFER,
    GATHER_PAGING_RESULT_VERSION, GATHER_RESULT_MAGIC, GATHER_RESULT_SIZE, GATHER_RESULT_VERSION,
    GATHER_STATUS_OFFSET, GatherFaultKind, GatherFaultResult, GatherPagingGeometry, GatherResult,
    decode_gather_address, decode_gather_paging_result, encode_gather_paging_result,
    encode_gather_result,
};

pub(crate) const AVX_ADDRESS_MAGIC: [u8; 4] = *b"R3AM";
pub(crate) const AVX_ADDRESS_VERSION: u16 = 1;
pub(crate) const AVX_ADDRESS_SEGMENT_OFFSET: usize = 16;
pub(crate) const AVX_ADDRESS_LOGICAL_OFFSET: usize = 20;
pub(crate) const AVX_ADDRESS_SIZE: usize = 24;
pub(crate) const AVX_ADDRESS_TRANSFER: usize = 544;
pub(crate) const AVX_OUTPUT_TRANSFER: usize = 512;
pub(crate) const AVX_OUTPUT_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AvxAddress {
    pub(crate) segment: SegmentRegister,
    pub(crate) offset: u32,
}

pub(crate) fn decode_avx_address(input: &[u8]) -> Result<AvxAddress, AbiError> {
    header::validate_versions(
        input,
        AVX_ADDRESS_MAGIC,
        AVX_ADDRESS_SIZE,
        &[AVX_ADDRESS_VERSION],
    )?;
    let segment = match header::read_u32(input, AVX_ADDRESS_SEGMENT_OFFSET) {
        0 => SegmentRegister::Es,
        1 => SegmentRegister::Cs,
        2 => SegmentRegister::Ss,
        3 => SegmentRegister::Ds,
        4 => SegmentRegister::Fs,
        5 => SegmentRegister::Gs,
        _ => return Err(AbiError::Reserved),
    };
    Ok(AvxAddress {
        segment,
        offset: header::read_u32(input, AVX_ADDRESS_LOGICAL_OFFSET),
    })
}

#[cfg(test)]
#[path = "avx/tests.rs"]
mod tests;
