use super::MemoryError;
use std::sync::atomic::{AtomicU64, Ordering};

// snapshots stay inside the engine instance that allocated their identity.
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

pub(crate) fn new_identity() -> Result<u64, MemoryError> {
    NEXT_IDENTITY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |identity| {
            identity.checked_add(1)
        })
        .map_err(|_| MemoryError::VersionExhausted)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageVersion {
    pub mapping: u64,
    pub content: u64,
}

#[derive(Debug)]
pub struct CodeSnapshot {
    pub(crate) identity: u64,
    pub(crate) first: usize,
    pub(crate) versions: Box<[PageVersion]>,
}
