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
