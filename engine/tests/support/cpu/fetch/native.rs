use ring3_engine::{
    cpu::dbt::{CompileError, InstructionError},
    cpu::x86::{CpuPagingError32, PagingAccess32, decode::DecodeError},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

#[derive(Debug, PartialEq, Eq)]
pub enum FetchFault {
    PageFault {
        pc: GuestAddress,
        linear: GuestAddress,
        access: PagingAccess32,
        error_code: u32,
        attempted_length: u32,
    },
    Memory {
        pc: GuestAddress,
        fault: MemoryFault,
        length: u32,
    },
}

pub fn observed(error: DecodeError) -> FetchFault {
    match error {
        DecodeError::Paging {
            pc,
            error: CpuPagingError32::PageFault(fault),
            attempted_length,
        } => FetchFault::PageFault {
            pc,
            linear: fault.linear(),
            access: fault.access(),
            error_code: fault.error_code(),
            attempted_length: u32::from(attempted_length),
        },
        DecodeError::MemoryFault { pc, fault, length } => FetchFault::Memory { pc, fault, length },
        other => panic!("expected precise code fetch fault, got {other:?}"),
    }
}

pub fn expected(pc: u32, address: u32, reason: FaultReason, length: u32) -> FetchFault {
    match reason {
        FaultReason::Unmapped | FaultReason::Permission => FetchFault::PageFault {
            pc: GuestAddress(pc),
            linear: GuestAddress(address),
            access: PagingAccess32::Execute,
            error_code: if reason == FaultReason::Permission {
                21
            } else {
                20
            },
            attempted_length: length,
        },
        _ => FetchFault::Memory {
            pc: GuestAddress(pc),
            fault: MemoryFault {
                address: GuestAddress(address),
                access: Access::Execute,
                reason,
            },
            length,
        },
    }
}

pub fn assert_error(error: DecodeError, pc: u32, address: u32, reason: FaultReason, length: u32) {
    assert_eq!(observed(error), expected(pc, address, reason, length));
}

#[allow(dead_code)]
pub fn assert_compile_error(
    error: CompileError,
    pc: u32,
    address: u32,
    reason: FaultReason,
    length: u32,
) {
    let CompileError::Instruction {
        pc: instruction_pc,
        cause: InstructionError::Decode(error),
    } = error
    else {
        panic!("expected an instruction decode fault, got {error:?}");
    };
    assert_eq!(instruction_pc, GuestAddress(pc));
    assert_error(error, pc, address, reason, length);
}

#[allow(dead_code)]
pub fn assert_host_error(
    error: HostError,
    pc: u32,
    address: u32,
    reason: FaultReason,
    length: u32,
) {
    let HostError::Compile(error) = error else {
        panic!("expected a process compilation fault, got {error:?}");
    };
    assert_compile_error(error, pc, address, reason, length);
}

#[allow(dead_code)]
pub fn assert_decode_contract(actual: DecodeError, expected_error: DecodeError) {
    if let DecodeError::MemoryFault {
        pc,
        fault:
            MemoryFault {
                address,
                access: Access::Execute,
                reason,
            },
        length,
    } = expected_error
    {
        assert_error(actual, pc.0, address.0, reason, length);
    } else {
        assert_eq!(actual, expected_error);
    }
}

#[allow(dead_code)]
pub fn assert_compile_contract(actual: CompileError, expected: CompileError) {
    if let CompileError::Instruction {
        pc,
        cause: InstructionError::Decode(expected),
    } = expected
    {
        let CompileError::Instruction {
            pc: actual_pc,
            cause: InstructionError::Decode(actual),
        } = actual
        else {
            panic!("expected precise instruction decode failure, got {actual:?}");
        };
        assert_eq!(actual_pc, pc);
        assert_decode_contract(actual, expected);
    } else {
        assert_eq!(actual, expected);
    }
}

#[allow(dead_code)]
pub fn assert_instruction_contract(
    actual: (GuestAddress, InstructionError),
    expected: (GuestAddress, InstructionError),
) {
    assert_eq!(actual.0, expected.0);
    match (actual.1, expected.1) {
        (InstructionError::Decode(actual), InstructionError::Decode(expected)) => {
            assert_decode_contract(actual, expected);
        }
        (actual, expected) => assert_eq!(actual, expected),
    }
}

#[allow(dead_code)]
pub fn assert_registry_contract(
    actual: ring3_engine::cpu::dbt::RegistryError,
    expected: ring3_engine::cpu::dbt::RegistryError,
) {
    use ring3_engine::cpu::dbt::RegistryError;
    if let RegistryError::Compile(expected) = expected {
        let RegistryError::Compile(actual) = actual else {
            panic!("expected registry compilation failure, got {actual:?}");
        };
        assert_compile_contract(actual, expected);
    } else {
        assert_eq!(actual, expected);
    }
}

#[allow(dead_code)]
pub fn assert_host_contract(actual: HostError, expected: HostError) {
    match expected {
        HostError::Compile(expected) => {
            let HostError::Compile(actual) = actual else {
                panic!("expected process compilation failure, got {actual:?}");
            };
            assert_compile_contract(actual, expected);
        }
        HostError::Resident(expected) => {
            let HostError::Resident(actual) = actual else {
                panic!("expected resident registry failure, got {actual:?}");
            };
            assert_registry_contract(actual, expected);
        }
        _ => assert_eq!(actual, expected),
    }
}
