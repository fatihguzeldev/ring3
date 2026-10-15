use crate::{
    files::{FileError32, FileId, FileReadRequest32, OpenFileId},
    memory::GuestAddress,
    windows::synchronization::EventBinding32,
};

pub(in crate::process::windows::files) struct AsyncRuntime32 {
    pub(super) operations: Vec<AsyncOperation32>,
    pub(super) max_operations: usize,
    pub(super) max_bytes: usize,
    pub(super) retained_bytes: usize,
    pub(super) next: u64,
    pub(super) wait: Option<(u64, u64)>,
}
pub(super) struct AsyncOperation32 {
    pub id: u64,
    pub open: OpenFileId,
    pub file: FileId,
    pub thread: u32,
    pub buffer: GuestAddress,
    pub length: u32,
    pub overlapped: GuestAddress,
    pub offset: u64,
    pub write: Option<Vec<u8>>,
    pub events: Vec<EventBinding32>,
    pub status: AsyncStatus32,
    pub cancel: bool,
    pub mappings: Vec<(GuestAddress, u64)>,
    pub lock: Option<AsyncLock32>,
}
#[derive(Clone, Copy)]
pub(super) struct AsyncLock32 {
    pub length: u64,
    pub exclusive: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AsyncStatus32 {
    Pending,
    Complete { error: u32, bytes: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum AsyncProgress32 {
    Idle,
    Completed {
        id: u64,
        open: OpenFileId,
        error: u32,
        bytes: u32,
    },
    NeedBytes(FileReadRequest32),
}
impl AsyncRuntime32 {
    pub(in crate::process::windows::files) fn new(
        max_operations: usize,
        max_bytes: usize,
    ) -> Result<Self, FileError32> {
        if max_operations == 0 || max_operations > 65_536 || max_bytes == 0 {
            return Err(FileError32::InvalidLimits);
        }
        let mut operations = Vec::new();
        operations
            .try_reserve_exact(max_operations)
            .map_err(|_| FileError32::Allocation)?;
        Ok(Self {
            operations,
            max_operations,
            max_bytes,
            retained_bytes: 0,
            next: 1,
            wait: None,
        })
    }
    pub(super) fn find(&self, open: OpenFileId, overlapped: GuestAddress) -> Option<usize> {
        self.operations
            .iter()
            .position(|operation| operation.open == open && operation.overlapped == overlapped)
    }
    pub(in crate::process::windows::files) fn is_file_signaled(&self, open: OpenFileId) -> bool {
        self.operations
            .iter()
            .filter(|op| op.open == open)
            .max_by_key(|op| op.id)
            .is_none_or(|op| matches!(op.status, AsyncStatus32::Complete { .. }))
    }
}

impl AsyncRuntime32 {
    pub(in crate::process::windows::files) fn cancel_open(&mut self, open: OpenFileId) {
        for operation in &mut self.operations {
            if operation.open == open && operation.status == AsyncStatus32::Pending {
                operation.cancel = true;
            }
        }
    }
}

impl AsyncRuntime32 {
    pub(in crate::process::windows::files) fn cancel_wait(&mut self) {
        self.wait = None;
    }
}
