mod paging;
use super::escalation::EscalationClass32;
pub use paging::{
    InvalidPageFault32, LegacyPageViolation32, PageFaultProfile32, PagingAccess32, PagingDomain32,
    VerifiedPageFault32,
};

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
    pub const fn null(external: bool) -> Self {
        Self(external as u16)
    }
    pub const fn descriptor(selector: u16, external: bool) -> Self {
        Self((selector & !3) | external as u16)
    }
    pub const fn idt(vector: u8, external: bool) -> Self {
        Self(((vector as u16) << 3) | 2 | external as u16)
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
    TaskSwitchTrap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionCause32 {
    DivideError,
    DoubleFault,
    Debug(DebugCause32),
    BreakpointInt3,
    OverflowInto,
    BoundRange,
    InvalidOpcode,
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
            Self::DoubleFault => 8,
            Self::Debug(_) => 1,
            Self::BreakpointInt3 => 3,
            Self::OverflowInto => 4,
            Self::BoundRange => 5,
            Self::InvalidOpcode => 6,
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
            Self::DoubleFault => ExceptionClass32::Abort,
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
            Self::DoubleFault => Some(0),
            Self::InvalidTss(code)
            | Self::SegmentNotPresent(code)
            | Self::StackSegment(code)
            | Self::GeneralProtection(code) => Some(code.get()),
            Self::PageFault(fault) => Some(fault.error_code()),
            _ => None,
        }
    }
    pub const fn escalation_class(self) -> EscalationClass32 {
        match self {
            Self::DoubleFault => EscalationClass32::DoubleFault,
            Self::DivideError
            | Self::InvalidTss(_)
            | Self::SegmentNotPresent(_)
            | Self::StackSegment(_)
            | Self::GeneralProtection(_) => EscalationClass32::Contributory,
            Self::PageFault(_) => EscalationClass32::PageFault,
            _ => EscalationClass32::Benign,
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
    pub const fn external(self) -> bool {
        !matches!(
            self,
            Self::SoftwareInt { .. }
                | Self::Exception(
                    ExceptionCause32::BreakpointInt3 | ExceptionCause32::OverflowInto
                )
        )
    }
    pub const fn checks_gate_dpl(self) -> bool {
        !self.external()
    }
    pub const fn error_code(self) -> Option<u32> {
        match self {
            Self::SoftwareInt { .. } => None,
            Self::Exception(cause) => cause.error_code(),
        }
    }
    pub const fn escalation_class(self) -> EscalationClass32 {
        match self {
            Self::SoftwareInt { .. } => EscalationClass32::Benign,
            Self::Exception(cause) => cause.escalation_class(),
        }
    }
    pub const fn saved_flags(self, flags: u32) -> u32 {
        match self {
            // selected diagnostic abort image; df does not inherit ordinary fault rf forcing.
            Self::Exception(ExceptionCause32::DoubleFault) => flags,
            Self::SoftwareInt { .. }
            | Self::Exception(
                ExceptionCause32::BreakpointInt3
                | ExceptionCause32::OverflowInto
                | ExceptionCause32::Debug(DebugCause32::Int1),
            ) => flags & !0x10000,
            Self::Exception(ExceptionCause32::Debug(
                DebugCause32::InstructionBreakpointFault | DebugCause32::TaskSwitchTrap,
            )) => flags,
            Self::Exception(ExceptionCause32::Debug(
                DebugCause32::SingleStep(point)
                | DebugCause32::DataBreakpoint(point)
                | DebugCause32::IoBreakpoint(point),
            )) => match point {
                TrapPoint32::CompletedInstruction => flags,
                TrapPoint32::RepetitionInProgress => flags | 0x10000,
            },
            Self::Exception(_) => flags | 0x10000,
        }
    }
}

#[cfg(test)]
mod tests;
