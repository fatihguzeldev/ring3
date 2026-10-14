use crate::memory::{Access, GuestAddress, MemoryFault};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedFeature {
    Opcode,
    FloatingPoint,
    Simd,
    Segment,
    RepeatedString,
    Privileged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfrastructureFailure {
    VersionExhausted,
    HelperProtocol,
    HelperRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    DeviceNotAvailable,
    Budget,
    Cancelled,
    NeedCode,
    Unsupported(UnsupportedFeature),
    MemoryFault {
        fault: MemoryFault,
        length: u32,
    },
    PageFault {
        error_code: u32,
        linear: GuestAddress,
        access: Access,
        length: u32,
    },
    CodeInvalidated,
    Infrastructure(InfrastructureFailure),
    Gate {
        id: u32,
    },
    ProcessExited {
        code: u32,
    },
    DivideError,
    DebugTrap,
    Breakpoint,
    Overflow,
    BoundRange,
    InvalidOpcode,
    GeneralProtection {
        error_code: u32,
    },
    FloatingPoint {
        status: u16,
    },
    SimdFloatingPoint {
        mxcsr: u32,
    },
    SegmentNotPresent {
        error_code: u32,
    },
    StackSegment {
        error_code: u32,
    },
    InvalidTss {
        error_code: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionExit {
    pub retired: u32,
    pub reason: ExitReason,
}
