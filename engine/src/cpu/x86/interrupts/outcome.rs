use super::cause::ExceptionCause32;
use crate::memory::{MemoryError, MemoryFault};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAuthorityKind32 {
    Task,
    Privilege,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnteredEvent32 {
    pub target: u32,
    pub esp: u32,
    pub flags: u32,
    pub frame_info: u32,
    pub code_invalidated: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventDeliveryResult32 {
    Entered(EnteredEvent32),
    ArchitecturalSecondary(ExceptionCause32),
    NeedsPagingAuthority {
        fault: MemoryFault,
        length: u32,
    },
    NeedsTaskOrPrivilegeAuthority {
        kind: EventAuthorityKind32,
        selector: u16,
    },
    Infrastructure(MemoryError),
}
