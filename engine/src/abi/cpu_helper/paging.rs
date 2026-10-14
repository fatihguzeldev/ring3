use crate::{
    abi::{
        AbiError, header,
        memory_helper::{CpuScalarOperation, HELPER_SIZE, encode_cpu_scalar_result},
    },
    cpu::x86::{CpuPagingError32, PagingAccess32, PagingDomain32},
    memory::GuestAddress,
};

// an actual native page fault, distinct from a successful or committed-smc helper.
pub const CPU_PAGE_FAULT_TRANSPORT: u32 = 23;
pub const CPU_PAGING_HELPER_VERSION: u16 = 15;

/// scalar paging errors carry architectural intent; legacy memory replies retain their old wire.
pub fn encode_cpu_paging_scalar_result(
    address: GuestAddress,
    width: u32,
    operation: CpuScalarOperation,
    result: Result<u32, CpuPagingError32>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    if !matches!(width, 1 | 2 | 4) {
        return Err(AbiError::MemoryHelper);
    }
    let error = match result {
        Ok(value) => return encode_cpu_scalar_result(address, width, operation, Ok(value), output),
        Err(CpuPagingError32::Memory(error)) => {
            return encode_cpu_scalar_result(address, width, operation, Err(error), output);
        }
        Err(error) => error,
    };
    let fields = fields(width, error)?;
    let access = match operation {
        CpuScalarOperation::Read => 1,
        CpuScalarOperation::Store | CpuScalarOperation::Modify => 2,
    };
    if fields[3].wrapping_sub(address.0) >= width || fields[4] & 0xff != access {
        return Err(AbiError::MemoryHelper);
    }
    publish(fields, output);
    Ok(())
}

pub(super) fn encode(
    length: u32,
    error: CpuPagingError32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = fields(length, error)?;
    publish(fields, output);
    Ok(())
}

pub(in crate::abi) fn encode_stack(
    slot: u32,
    error: CpuPagingError32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = stack_fields(slot, error)?;
    publish(fields, output);
    Ok(())
}

fn stack_fields(slot: u32, error: CpuPagingError32) -> Result<[u32; 6], AbiError> {
    if !matches!(slot, 2 | 4) {
        return Err(AbiError::MemoryHelper);
    }
    let valid = match error {
        CpuPagingError32::PageFault(fault) => {
            fault.access() == PagingAccess32::Write && fault.domain() == PagingDomain32::User
        }
        CpuPagingError32::Memory(_) => false,
    };
    if !valid {
        return Err(AbiError::MemoryHelper);
    }
    fields(slot, error)
}

pub(in crate::abi) fn fields(length: u32, error: CpuPagingError32) -> Result<[u32; 6], AbiError> {
    Ok(match error {
        CpuPagingError32::PageFault(fault) => {
            let code = fault.error_code();
            let valid = match fault.access() {
                PagingAccess32::Read => matches!(code, 4 | 5),
                PagingAccess32::Write => matches!(code, 6 | 7),
                PagingAccess32::Execute => matches!(code, 20 | 21),
            };
            if !valid {
                return Err(AbiError::MemoryHelper);
            }
            [9, 0, code, fault.linear().0, access(fault.access()), length]
        }
        CpuPagingError32::Memory(_) => return Err(AbiError::MemoryHelper),
    })
}

fn access(access: PagingAccess32) -> u32 {
    match access {
        PagingAccess32::Read => 1,
        PagingAccess32::Write => 2,
        PagingAccess32::Execute => 3,
    }
}

pub(super) fn publish(fields: [u32; 6], output: &mut [u8]) {
    publish_version(fields, CPU_PAGING_HELPER_VERSION, output);
}

fn publish_version(fields: [u32; 6], version: u16, output: &mut [u8]) {
    header::write_version(output, *b"R3MH", version);
    for (index, value) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, value);
    }
}

#[cfg(test)]
mod tests;
