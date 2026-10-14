mod table;

pub use table::InterruptDescriptorTable32;

mod gates;

mod cause;
mod escalation;
mod frame;
mod outcome;

pub use cause::{
    DebugCause32, EventOrigin32, ExceptionCause32, ExceptionClass32, InvalidPageFault32,
    LegacyPageViolation32, PageFaultProfile32, PagingAccess32, PagingDomain32, SelectorErrorCode32,
    TrapPoint32, VerifiedPageFault32,
};
pub use escalation::{
    EscalationClass32, EscalationDecision32, SecondaryClass32, escalate_exception,
};
pub(crate) use frame::EventFrame32;
pub use frame::FrameInput32;
pub use outcome::{
    DetectedEntryResult32, EnteredEvent32, EventAuthorityKind32, EventDeliveryResult32,
};

pub(crate) use gates::secondary_error as event_secondary_error;
