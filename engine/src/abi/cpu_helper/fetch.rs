use super::{AbiError, CpuMemoryError, encode_cpu_result, paging};
use crate::{
    cpu::x86::{CpuPagingError32, PagingAccess32, PagingDomain32},
    memory::{Access, FaultReason, GuestAddress, MemoryError},
};

pub(crate) fn encode_cpu_fetch_result(
    linear: GuestAddress,
    length: u32,
    result: Result<(), CpuMemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != super::HELPER_SIZE {
        return Err(AbiError::Length);
    }
    if !(1..=15).contains(&length) {
        return Err(AbiError::MemoryHelper);
    }
    let fields = match result {
        Err(CpuMemoryError::Memory(MemoryError::Fault(fault))) => {
            if fault.access != Access::Execute || fault.address.0.wrapping_sub(linear.0) >= length {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => return Err(AbiError::MemoryHelper),
            };
            [1, 0, detail, fault.address.0, 3, length]
        }
        Err(CpuMemoryError::Paging(error)) => {
            let valid = match error {
                CpuPagingError32::PageFault(fault) => {
                    fault.access() == PagingAccess32::Execute
                        && fault.domain() == PagingDomain32::User
                }
                _ => false,
            };
            if !valid {
                return Err(AbiError::MemoryHelper);
            }
            let fields = paging::fields(length, error)?;
            if fields[3].wrapping_sub(linear.0) >= length {
                return Err(AbiError::MemoryHelper);
            }
            fields
        }
        other => return encode_cpu_result(length, 0, other, output),
    };
    paging::publish(fields, output);
    Ok(())
}
