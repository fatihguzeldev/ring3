use crate::abi::{AbiError, header};
use crate::memory::{Access, FaultReason, GuestAddress, MemoryError};

pub const CPU_SCALAR_HELPER_VERSION: u16 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuScalarOperation {
    Read,
    Store,
    Modify,
}

/// encode an explicit CPU linear-bus result; legacy scalar codecs remain strict.
pub fn encode_cpu_scalar_result(
    address: GuestAddress,
    width: u32,
    operation: CpuScalarOperation,
    result: Result<u32, MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != super::HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let maximum = match width {
        1 => 0xff,
        2 => 0xffff,
        4 => u32::MAX,
        _ => return Err(AbiError::MemoryHelper),
    };
    let fields = match result {
        Ok(value) => {
            if value > maximum || (operation == CpuScalarOperation::Store && value != 0) {
                return Err(AbiError::MemoryHelper);
            }
            [0, value, 0, 0, 0, width]
        }
        Err(MemoryError::Fault(fault)) => {
            let permitted = matches!(
                (operation, fault.access, fault.reason),
                (
                    CpuScalarOperation::Read | CpuScalarOperation::Modify,
                    Access::Read,
                    FaultReason::Unmapped | FaultReason::Permission,
                ) | (
                    CpuScalarOperation::Store,
                    Access::Write,
                    FaultReason::Unmapped | FaultReason::Permission,
                ) | (
                    CpuScalarOperation::Modify,
                    Access::Write,
                    FaultReason::Permission
                )
            );
            if !permitted || fault.address.0.wrapping_sub(address.0) >= width {
                return Err(AbiError::MemoryHelper);
            }
            [
                1,
                0,
                if fault.reason == FaultReason::Unmapped {
                    1
                } else {
                    2
                },
                fault.address.0,
                if fault.access == Access::Read { 1 } else { 2 },
                width,
            ]
        }
        Err(error) => [
            2,
            0,
            if error == MemoryError::VersionExhausted {
                1
            } else {
                2
            },
            0,
            0,
            width,
        ],
    };
    header::write_version(output, *b"R3MH", CPU_SCALAR_HELPER_VERSION);
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
