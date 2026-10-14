pub mod atomic;
pub(crate) mod avx;
pub mod bits;
pub(crate) mod capabilities;
mod control;
pub mod decode;
mod environment;
mod interrupts;
pub mod ir;
pub mod packed_strings;
mod paging;
mod segments;
pub mod simd;
mod stack;
mod state;
mod system_registers;
mod x87;
pub mod xstate;

pub use avx::AvxOperation;
pub use control::{
    ControlOperation, USER_IRET_FLAGS_MASK, USER_IRET_FLAGS_WORD_MASK, USER_POP_FLAGS_MASK,
    USER_POP_FLAGS_WORD_MASK, interrupt_return_flags, pop_flags,
};
pub(crate) use environment::USER_MODE_ENVIRONMENT;
pub use interrupts::{
    DebugCause32, EventOrigin32, ExceptionCause32, ExceptionClass32, FrameInput32, PagingAccess32,
    PagingDomain32, SelectorErrorCode32, TrapPoint32, VerifiedPageFault32,
};
pub use paging::CpuPagingError32;
pub(crate) use paging::{CpuMemoryPort32, CpuReadPort32};
pub use segments::{
    CodeDestination, CodeTransfer, CodeTransferError, FarWidth, SegmentAccess, SegmentAccessError,
    SegmentCache, SegmentContext, SegmentDescriptor, SegmentDescriptorKind,
    SegmentDescriptorRecord, SegmentDescriptorTable, SegmentLoadError, SegmentLoadFailure,
    SegmentQuery, SegmentRegister, SegmentState,
};
pub use stack::StackFrameKind;
pub(crate) use stack::StackPointerSize;
pub use state::{Register32, State32};
pub use system_registers::{
    DescriptorTableRegister32, SystemRegisterQuery, UserSystemRegisterBank32,
};
pub use x87::{
    Ext80, X87Binary, X87Condition, X87Error, X87Format, X87Operation, X87Outcome, X87Rounding,
    X87State,
};

pub(crate) use paging::inspect_cpu_paging_access32;

#[cfg(test)]
pub(crate) use paging::{DefaultCpuMemoryPort32, DefaultCpuReadPort32};
