use crate::abi::header::{self, read_u32, write_u32};
use crate::abi::{ABI_VERSION, AbiError};
use crate::cpu::{ExecutionExit, ExitReason, InfrastructureFailure, UnsupportedFeature};
use crate::memory::{Access, FaultReason, GuestAddress, MemoryFault};

pub const EXIT_SIZE: usize = 40;
pub const EXIT_VERSION_2: u16 = 2;
pub const EXIT_VERSION_3: u16 = 3;
pub const EXIT_VERSION_4: u16 = 4;
pub const EXIT_VERSION_5: u16 = 5;
pub const REASON_OFFSET: usize = 16;
pub const RETIRED_OFFSET: usize = 20;
pub const DETAIL_OFFSET: usize = 24;
pub const FAULT_ADDRESS_OFFSET: usize = 28;
pub const ACCESS_OFFSET: usize = 32;
pub const ACCESS_LENGTH_OFFSET: usize = 36;

pub fn encode_exit(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, ABI_VERSION)
}

pub fn encode_exit_v2(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_2)
}

pub fn encode_exit_v3(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_3)
}

pub fn encode_exit_v4(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_4)
}

pub fn encode_exit_v5(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_5)
}

fn encode(exit: &ExecutionExit, output: &mut [u8], version: u16) -> Result<(), AbiError> {
    if output.len() != EXIT_SIZE {
        return Err(AbiError::Length);
    }
    let mut fields = [0; 6];
    fields[1] = exit.retired;
    fields[0] = match exit.reason {
        ExitReason::Budget => 1,
        ExitReason::Cancelled => 2,
        ExitReason::NeedCode => 3,
        ExitReason::Unsupported(feature) => {
            fields[2] = feature_code(feature);
            4
        }
        ExitReason::MemoryFault { fault, length } => {
            validate_fault(fault, length)?;
            fields[2] = match fault.reason {
                FaultReason::Unmapped => 1,
                FaultReason::Permission => 2,
                FaultReason::AddressOverflow => 3,
            };
            fields[3] = fault.address.0;
            fields[4] = match fault.access {
                Access::Read => 1,
                Access::Write => 2,
                Access::Execute => 3,
            };
            fields[5] = length;
            5
        }
        ExitReason::CodeInvalidated => 6,
        ExitReason::Infrastructure(reason) => {
            if !matches!(
                version,
                EXIT_VERSION_2 | EXIT_VERSION_3 | EXIT_VERSION_4 | EXIT_VERSION_5
            ) {
                return Err(AbiError::Exit);
            }
            fields[2] = match reason {
                InfrastructureFailure::VersionExhausted => 1,
                InfrastructureFailure::HelperProtocol => 2,
                InfrastructureFailure::HelperRejected => 3,
            };
            7
        }
        ExitReason::Gate { id } => {
            if !matches!(version, EXIT_VERSION_3 | EXIT_VERSION_4 | EXIT_VERSION_5) || id == 0 {
                return Err(AbiError::Exit);
            }
            fields[2] = id;
            8
        }
        ExitReason::ProcessExited { code } => {
            if !matches!(version, EXIT_VERSION_4 | EXIT_VERSION_5) {
                return Err(AbiError::Exit);
            }
            fields[2] = code;
            9
        }
        ExitReason::DivideError => {
            if version != EXIT_VERSION_5 {
                return Err(AbiError::Exit);
            }
            10
        }
    };
    header::write_version(output, *b"R3EX", version);
    for (index, value) in fields.iter().enumerate() {
        write_u32(output, REASON_OFFSET + index * 4, *value);
    }
    Ok(())
}

pub fn decode_exit(input: &[u8]) -> Result<ExecutionExit, AbiError> {
    let version = header::validate_versions(
        input,
        *b"R3EX",
        EXIT_SIZE,
        &[
            ABI_VERSION,
            EXIT_VERSION_2,
            EXIT_VERSION_3,
            EXIT_VERSION_4,
            EXIT_VERSION_5,
        ],
    )?;
    let detail = read_u32(input, DETAIL_OFFSET);
    let address = read_u32(input, FAULT_ADDRESS_OFFSET);
    let access = read_u32(input, ACCESS_OFFSET);
    let length = read_u32(input, ACCESS_LENGTH_OFFSET);
    let tag = read_u32(input, REASON_OFFSET);
    let reason = if tag == 5 {
        let fault = MemoryFault {
            address: GuestAddress(address),
            access: match access {
                1 => Access::Read,
                2 => Access::Write,
                3 => Access::Execute,
                _ => return Err(AbiError::Exit),
            },
            reason: match detail {
                1 => FaultReason::Unmapped,
                2 => FaultReason::Permission,
                3 => FaultReason::AddressOverflow,
                _ => return Err(AbiError::Exit),
            },
        };
        validate_fault(fault, length)?;
        ExitReason::MemoryFault { fault, length }
    } else {
        if address != 0
            || access != 0
            || length != 0
            || (!matches!(tag, 4 | 7 | 8 | 9) && detail != 0)
        {
            return Err(AbiError::Exit);
        }
        match tag {
            1 => ExitReason::Budget,
            2 => ExitReason::Cancelled,
            3 => ExitReason::NeedCode,
            4 => ExitReason::Unsupported(decode_feature(detail)?),
            6 => ExitReason::CodeInvalidated,
            7 if matches!(
                version,
                EXIT_VERSION_2 | EXIT_VERSION_3 | EXIT_VERSION_4 | EXIT_VERSION_5
            ) =>
            {
                ExitReason::Infrastructure(match detail {
                    1 => InfrastructureFailure::VersionExhausted,
                    2 => InfrastructureFailure::HelperProtocol,
                    3 => InfrastructureFailure::HelperRejected,
                    _ => return Err(AbiError::Exit),
                })
            }
            8 if matches!(version, EXIT_VERSION_3 | EXIT_VERSION_4 | EXIT_VERSION_5)
                && detail != 0 =>
            {
                ExitReason::Gate { id: detail }
            }
            9 if matches!(version, EXIT_VERSION_4 | EXIT_VERSION_5) => {
                ExitReason::ProcessExited { code: detail }
            }
            10 if version == EXIT_VERSION_5 => ExitReason::DivideError,
            _ => return Err(AbiError::Exit),
        }
    };
    Ok(ExecutionExit {
        retired: read_u32(input, RETIRED_OFFSET),
        reason,
    })
}

fn validate_fault(fault: MemoryFault, length: u32) -> Result<(), AbiError> {
    let valid_length = match fault.access {
        Access::Read | Access::Write => matches!(length, 1 | 2 | 4),
        Access::Execute => (1..=15).contains(&length),
    };
    let overflows = u64::from(fault.address.0) + u64::from(length) > 1 << 32;
    if !valid_length || overflows != (fault.reason == FaultReason::AddressOverflow) {
        return Err(AbiError::Exit);
    }
    Ok(())
}

fn feature_code(feature: UnsupportedFeature) -> u32 {
    match feature {
        UnsupportedFeature::Opcode => 1,
        UnsupportedFeature::FloatingPoint => 2,
        UnsupportedFeature::Simd => 3,
        UnsupportedFeature::Segment => 4,
        UnsupportedFeature::RepeatedString => 5,
        UnsupportedFeature::Privileged => 6,
    }
}

fn decode_feature(code: u32) -> Result<UnsupportedFeature, AbiError> {
    Ok(match code {
        1 => UnsupportedFeature::Opcode,
        2 => UnsupportedFeature::FloatingPoint,
        3 => UnsupportedFeature::Simd,
        4 => UnsupportedFeature::Segment,
        5 => UnsupportedFeature::RepeatedString,
        6 => UnsupportedFeature::Privileged,
        _ => return Err(AbiError::Exit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::ExitReason;

    #[test]
    fn budget_exit_has_a_binary_roundtrip() {
        let exit = ExecutionExit {
            retired: 4,
            reason: ExitReason::Budget,
        };
        let mut bytes = [0; EXIT_SIZE];
        encode_exit(&exit, &mut bytes).unwrap();
        assert_eq!(decode_exit(&bytes).unwrap(), exit);
    }
}
