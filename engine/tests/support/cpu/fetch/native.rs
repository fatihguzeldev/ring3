use ring3_engine::{
    cpu::x86::{CpuPagingError32, PagingAccess32, decode::DecodeError},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
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
