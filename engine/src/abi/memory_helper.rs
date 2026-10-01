#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::memory::{Access, FaultReason, MemoryError};

pub const HELPER_SIZE: usize = 40;

pub fn encode_helper_result(
    result: Result<u32, MemoryError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let fields = match result {
        Ok(value) => [0, value, 0, 0, 0, 0],
        Err(MemoryError::Fault(fault)) => {
            let access = match fault.access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => return Err(AbiError::MemoryHelper),
            };
            let overflows = u64::from(fault.address.0) + 4 > 1 << 32;
            if overflows != (fault.reason == FaultReason::AddressOverflow) {
                return Err(AbiError::MemoryHelper);
            }
            let detail = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            };
            [1, 0, detail, fault.address.0, access, 4]
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
            0,
        ],
    };
    header::write(output, *b"R3MH");
    for (index, field) in fields.into_iter().enumerate() {
        header::write_u32(output, 16 + index * 4, field);
    }
    Ok(())
}
