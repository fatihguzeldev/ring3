mod paging;
pub use paging::{PagingAccess32, PagingDomain32, VerifiedPageFault32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectorErrorCode32(u16);
impl SelectorErrorCode32 {
    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits <= u16::MAX as u32 {
            Some(Self(bits as u16))
        } else {
            None
        }
    }
    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapPoint32 {
    CompletedInstruction,
    RepetitionInProgress,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugCause32 {
    Int1,
    SingleStep(TrapPoint32),
    DataBreakpoint(TrapPoint32),
    IoBreakpoint(TrapPoint32),
    InstructionBreakpointFault,
    GeneralDetectFault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionCause32 {
    DivideError,
    Debug(DebugCause32),
    BreakpointInt3,
    OverflowInto,
    BoundRange,
    InvalidOpcode,
    DeviceNotAvailable,
    InvalidTss(SelectorErrorCode32),
    SegmentNotPresent(SelectorErrorCode32),
    StackSegment(SelectorErrorCode32),
    GeneralProtection(SelectorErrorCode32),
    PageFault(VerifiedPageFault32),
    FloatingPoint { status: u16 },
    SimdFloatingPoint { mxcsr: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventOrigin32 {
    SoftwareInt { vector: u8 },
    Exception(ExceptionCause32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionClass32 {
    Fault,
    Trap,
    Abort,
}

impl ExceptionCause32 {
    pub const fn vector(self) -> u8 {
        match self {
            Self::DivideError => 0,
            Self::Debug(_) => 1,
            Self::BreakpointInt3 => 3,
            Self::OverflowInto => 4,
            Self::BoundRange => 5,
            Self::InvalidOpcode => 6,
            Self::DeviceNotAvailable => 7,
            Self::InvalidTss(_) => 10,
            Self::SegmentNotPresent(_) => 11,
            Self::StackSegment(_) => 12,
            Self::GeneralProtection(_) => 13,
            Self::PageFault(_) => 14,
            Self::FloatingPoint { .. } => 16,
            Self::SimdFloatingPoint { .. } => 19,
        }
    }
    pub const fn class(self) -> ExceptionClass32 {
        match self {
            Self::BreakpointInt3 | Self::OverflowInto => ExceptionClass32::Trap,
            Self::Debug(
                DebugCause32::InstructionBreakpointFault | DebugCause32::GeneralDetectFault,
            ) => ExceptionClass32::Fault,
            Self::Debug(_) => ExceptionClass32::Trap,
            _ => ExceptionClass32::Fault,
        }
    }
    pub const fn error_code(self) -> Option<u32> {
        match self {
            Self::InvalidTss(code)
            | Self::SegmentNotPresent(code)
            | Self::StackSegment(code)
            | Self::GeneralProtection(code) => Some(code.get()),
            Self::PageFault(fault) => Some(fault.error_code()),
            _ => None,
        }
    }
}
impl EventOrigin32 {
    pub const fn vector(self) -> u8 {
        match self {
            Self::SoftwareInt { vector } => vector,
            Self::Exception(cause) => cause.vector(),
        }
    }
    pub const fn error_code(self) -> Option<u32> {
        match self {
            Self::SoftwareInt { .. } => None,
            Self::Exception(cause) => cause.error_code(),
        }
    }
}

#[cfg(test)]
mod tests;
