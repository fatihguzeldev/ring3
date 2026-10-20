mod address;
mod code;
mod paging;
mod space;

pub use address::{
    Access, BackingOffset, FaultReason, GuestAddress, MAX_RESIDENT_PAGES, MAX_WORD_WRITES32,
    MemoryError, MemoryFault, PAGE_SIZE, PageRange, Permissions, WordWrite32,
};
pub use code::CodeSnapshot;
pub use space::AddressSpace;
pub(crate) use space::{CoreMemoryView32, CoreWriteOutcome};
pub(crate) use space::{GuardPage32, PageInitialization, PrivatePageChange32, SharedPages};

pub(crate) use paging::{PagingBacking32, PagingCheckError32};
