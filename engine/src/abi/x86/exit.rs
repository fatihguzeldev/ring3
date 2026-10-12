use crate::abi::header::{self, read_u32, write_u32};
use crate::abi::{ABI_VERSION, AbiError};
use crate::cpu::{ExecutionExit, ExitReason, InfrastructureFailure, UnsupportedFeature};
use crate::memory::{Access, FaultReason, GuestAddress, MemoryFault};

pub const EXIT_SIZE: usize = 40;
pub const EXIT_VERSION_2: u16 = 2;
pub const EXIT_VERSION_3: u16 = 3;
pub const EXIT_VERSION_4: u16 = 4;
pub const EXIT_VERSION_5: u16 = 5;
pub const EXIT_VERSION_6: u16 = 6;
pub const EXIT_VERSION_7: u16 = 7;
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

pub fn encode_exit_v6(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_6)
}

pub fn encode_exit_v7(exit: &ExecutionExit, output: &mut [u8]) -> Result<(), AbiError> {
    encode(exit, output, EXIT_VERSION_7)
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
            validate_fault(fault, length, version)?;
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
            if version < EXIT_VERSION_2 {
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
            if version < EXIT_VERSION_3 || id == 0 {
                return Err(AbiError::Exit);
            }
            fields[2] = id;
            8
        }
        ExitReason::ProcessExited { code } => {
            if version < EXIT_VERSION_4 {
                return Err(AbiError::Exit);
            }
            fields[2] = code;
            9
        }
        ExitReason::DivideError => {
            if version < EXIT_VERSION_5 {
                return Err(AbiError::Exit);
            }
            10
        }
        ExitReason::GeneralProtection { error_code } => {
            if version < EXIT_VERSION_6 {
                return Err(AbiError::Exit);
            }
            fields[2] = error_code;
            11
        }
        ExitReason::FloatingPoint { status } => {
            if version < EXIT_VERSION_6 {
                return Err(AbiError::Exit);
            }
            fields[2] = u32::from(status);
            12
        }
        ExitReason::SimdFloatingPoint { mxcsr } => {
            if version < EXIT_VERSION_6 || mxcsr & !0xffff != 0 {
                return Err(AbiError::Exit);
            }
            fields[2] = mxcsr;
            13
        }
        ExitReason::SegmentNotPresent { error_code } | ExitReason::StackSegment { error_code } => {
            if version < EXIT_VERSION_6 {
                return Err(AbiError::Exit);
            }
            fields[2] = error_code;
            if matches!(exit.reason, ExitReason::SegmentNotPresent { .. }) {
                14
            } else {
                15
            }
        }
        ExitReason::DebugTrap
        | ExitReason::Breakpoint
        | ExitReason::Overflow
        | ExitReason::BoundRange
        | ExitReason::InvalidOpcode => {
            if version < EXIT_VERSION_7 {
                return Err(AbiError::Exit);
            }
            match exit.reason {
                ExitReason::DebugTrap => 16,
                ExitReason::Breakpoint => 17,
                ExitReason::Overflow => 18,
                ExitReason::BoundRange => 19,
                ExitReason::InvalidOpcode => 20,
                _ => unreachable!(),
            }
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
            EXIT_VERSION_6,
            EXIT_VERSION_7,
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
        validate_fault(fault, length, version)?;
        ExitReason::MemoryFault { fault, length }
    } else {
        if address != 0
            || access != 0
            || length != 0
            || (!matches!(tag, 4 | 7 | 8 | 9 | 11 | 12 | 13 | 14 | 15) && detail != 0)
        {
            return Err(AbiError::Exit);
        }
        match tag {
            1 => ExitReason::Budget,
            2 => ExitReason::Cancelled,
            3 => ExitReason::NeedCode,
            4 => ExitReason::Unsupported(decode_feature(detail)?),
            6 => ExitReason::CodeInvalidated,
            7 if version >= EXIT_VERSION_2 => ExitReason::Infrastructure(match detail {
                1 => InfrastructureFailure::VersionExhausted,
                2 => InfrastructureFailure::HelperProtocol,
                3 => InfrastructureFailure::HelperRejected,
                _ => return Err(AbiError::Exit),
            }),
            8 if version >= EXIT_VERSION_3 && detail != 0 => ExitReason::Gate { id: detail },
            9 if version >= EXIT_VERSION_4 => ExitReason::ProcessExited { code: detail },
            10 if version >= EXIT_VERSION_5 => ExitReason::DivideError,
            11 if version >= EXIT_VERSION_6 => ExitReason::GeneralProtection { error_code: detail },
            12 if version >= EXIT_VERSION_6 && detail <= 0xffff => ExitReason::FloatingPoint {
                status: detail as u16,
            },
            13 if version >= EXIT_VERSION_6 && detail <= 0xffff => {
                ExitReason::SimdFloatingPoint { mxcsr: detail }
            }
            14 if version >= EXIT_VERSION_6 => ExitReason::SegmentNotPresent { error_code: detail },
            15 if version >= EXIT_VERSION_6 => ExitReason::StackSegment { error_code: detail },
            16 if version >= EXIT_VERSION_7 => ExitReason::DebugTrap,
            17 if version >= EXIT_VERSION_7 => ExitReason::Breakpoint,
            18 if version >= EXIT_VERSION_7 => ExitReason::Overflow,
            19 if version >= EXIT_VERSION_7 => ExitReason::BoundRange,
            20 if version >= EXIT_VERSION_7 => ExitReason::InvalidOpcode,
            _ => return Err(AbiError::Exit),
        }
    };
    Ok(ExecutionExit {
        retired: read_u32(input, RETIRED_OFFSET),
        reason,
    })
}

fn validate_fault(fault: MemoryFault, length: u32, version: u16) -> Result<(), AbiError> {
    let valid_length = match fault.access {
        Access::Read | Access::Write if version >= EXIT_VERSION_6 => (1..=512).contains(&length),
        Access::Read | Access::Write => matches!(length, 1 | 2 | 4),
        Access::Execute => (1..=15).contains(&length),
    };
    let overflows = u64::from(fault.address.0) + u64::from(length) > 1 << 32;
    if !valid_length
        || ((version < EXIT_VERSION_6 || fault.reason == FaultReason::AddressOverflow)
            && overflows != (fault.reason == FaultReason::AddressOverflow))
    {
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

    #[test]
    fn architectural_traps_require_version_seven_and_empty_details() {
        for reason in [
            ExitReason::DebugTrap,
            ExitReason::Breakpoint,
            ExitReason::Overflow,
            ExitReason::BoundRange,
            ExitReason::InvalidOpcode,
        ] {
            let exit = ExecutionExit { retired: 1, reason };
            let mut bytes = [0; EXIT_SIZE];
            assert_eq!(encode_exit_v6(&exit, &mut bytes), Err(AbiError::Exit));
            encode_exit_v7(&exit, &mut bytes).unwrap();
            assert_eq!(decode_exit(&bytes).unwrap(), exit);
            bytes[DETAIL_OFFSET] = 1;
            assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
        }
    }
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

    #[test]
    fn cpu_exceptions_require_version6_and_preserve_exact_details() {
        for reason in [
            ExitReason::GeneralProtection { error_code: 0x43 },
            ExitReason::FloatingPoint { status: 0x8081 },
            ExitReason::SimdFloatingPoint { mxcsr: 0x1f81 },
            ExitReason::SegmentNotPresent { error_code: 0x48 },
            ExitReason::StackSegment { error_code: 0x50 },
        ] {
            let exit = ExecutionExit { reason, retired: 3 };
            let mut bytes = [0; EXIT_SIZE];
            assert_eq!(encode_exit_v5(&exit, &mut bytes), Err(AbiError::Exit));
            encode_exit_v6(&exit, &mut bytes).unwrap();
            assert_eq!(decode_exit(&bytes), Ok(exit));
            bytes[4] = 5;
            assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
        }
    }

    #[test]
    fn version6_preserves_large_spans_and_rejects_malformed_faults() {
        let exit = ExecutionExit {
            retired: 0,
            reason: ExitReason::MemoryFault {
                fault: MemoryFault {
                    address: GuestAddress(0x3000),
                    access: Access::Write,
                    reason: FaultReason::Unmapped,
                },
                length: 512,
            },
        };
        let mut bytes = [0; EXIT_SIZE];
        encode_exit_v6(&exit, &mut bytes).unwrap();
        assert_eq!(decode_exit(&bytes), Ok(exit));
        write_u32(&mut bytes, ACCESS_LENGTH_OFFSET, 513);
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
        write_u32(&mut bytes, ACCESS_LENGTH_OFFSET, 0);
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
    }
}
