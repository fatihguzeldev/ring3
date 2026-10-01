use crate::memory::MemoryFault;

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
pub enum ExitReason {
    Budget,
    Cancelled,
    NeedCode,
    Unsupported(UnsupportedFeature),
    MemoryFault { fault: MemoryFault, length: u32 },
    CodeInvalidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionExit {
    pub retired: u32,
    pub reason: ExitReason,
}
