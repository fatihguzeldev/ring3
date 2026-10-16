use crate::{
    files::{FileError32, FileId, FileReadRequest32, OpenFileId},
    memory::GuestAddress,
    windows::synchronization::EventBinding32,
};

pub(in crate::process::windows::files) struct AsyncRuntime32 {
    pub(in crate::process::windows::files) operations: Vec<AsyncOperation32>,
    pub(in crate::process::windows::files) max_operations: usize,
    pub(in crate::process::windows::files) max_bytes: usize,
    pub(in crate::process::windows::files) retained_bytes: usize,
    pub(in crate::process::windows::files) next: u64,
    pub(in crate::process::windows::files) wait: Option<(u64, u64)>,
    pub(in crate::process::windows::files) interrupted: bool,
    pub(in crate::process::windows::files) blocking: Option<u64>,
}
pub(in crate::process::windows::files) struct AsyncOperation32 {
    pub id: u64,
    pub target: AsyncTarget32,
    pub thread: u32,
    pub buffer: GuestAddress,
    pub length: u32,
    pub overlapped: Option<GuestAddress>,
    pub count_pointer: Option<GuestAddress>,
    pub offset: u64,
    pub kind: AsyncKind32,
    pub events: Vec<EventBinding32>,
    pub status: AsyncStatus32,
    pub cancel: bool,
    pub mappings: Vec<(GuestAddress, u64)>,
    pub skip_port: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process::windows::files) enum AsyncTarget32 {
    File { open: OpenFileId, file: FileId },
    Pipe(crate::windows::files::pipes::PipeEndpointId32),
}
impl AsyncTarget32 {
    pub(in crate::process::windows::files) fn open(self) -> Option<OpenFileId> {
        if let Self::File { open, .. } = self {
            Some(open)
        } else {
            None
        }
    }
    pub(in crate::process::windows::files) fn file(self) -> Option<(OpenFileId, FileId)> {
        if let Self::File { open, file } = self {
            Some((open, file))
        } else {
            None
        }
    }
    pub(in crate::process::windows::files) fn description(
        self,
    ) -> crate::windows::files::completion::CompletionTarget32 {
        match self {
            Self::File { open, .. } => {
                crate::windows::files::completion::CompletionTarget32::File(open)
            }
            Self::Pipe(pipe) => crate::windows::files::completion::CompletionTarget32::Pipe(pipe),
        }
    }
}
pub(in crate::process::windows::files) enum AsyncKind32 {
    Read,
    Write(Vec<u8>),
    Lock(AsyncLock32),
    Connect,
    Transact {
        input: Vec<u8>,
        sent: bool,
        close: bool,
    },
}
impl AsyncOperation32 {
    pub(in crate::process::windows::files) fn pipe_release_count(&self) -> u32 {
        if matches!(self.kind, AsyncKind32::Transact { close: true, .. }) {
            2
        } else {
            1
        }
    }
    pub(in crate::process::windows::files) fn write_data(&self) -> Option<&[u8]> {
        match &self.kind {
            AsyncKind32::Write(bytes) => Some(bytes),
            AsyncKind32::Transact {
                input, sent: false, ..
            } => Some(input),
            _ => None,
        }
    }
    pub(in crate::process::windows::files) fn lock(&self) -> Option<AsyncLock32> {
        if let AsyncKind32::Lock(lock) = self.kind {
            Some(lock)
        } else {
            None
        }
    }
    pub(in crate::process::windows::files) fn take_data(&mut self) -> Option<Vec<u8>> {
        match std::mem::replace(&mut self.kind, AsyncKind32::Read) {
            AsyncKind32::Write(bytes) | AsyncKind32::Transact { input: bytes, .. } => Some(bytes),
            _ => None,
        }
    }
}
#[derive(Clone, Copy)]
pub(in crate::process::windows::files) struct AsyncLock32 {
    pub length: u64,
    pub exclusive: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process::windows::files) enum AsyncStatus32 {
    Pending,
    Complete { error: u32, bytes: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum AsyncProgress32 {
    Idle,
    Completed {
        id: u64,
        target: crate::windows::files::completion::CompletionTarget32,
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
            interrupted: false,
            blocking: None,
        })
    }
    pub(super) fn find(&self, open: OpenFileId, overlapped: GuestAddress) -> Option<usize> {
        self.find_description(
            crate::windows::files::completion::CompletionTarget32::File(open),
            overlapped,
        )
    }
    pub(in crate::process::windows::files) fn find_description(
        &self,
        target: crate::windows::files::completion::CompletionTarget32,
        overlapped: GuestAddress,
    ) -> Option<usize> {
        self.operations.iter().position(|operation| {
            operation.target.description() == target && operation.overlapped == Some(overlapped)
        })
    }
    pub(in crate::process::windows::files) fn is_description_signaled(
        &self,
        target: crate::windows::files::completion::CompletionTarget32,
    ) -> bool {
        self.operations
            .iter()
            .filter(|op| op.target.description() == target)
            .max_by_key(|op| op.id)
            .is_none_or(|op| matches!(op.status, AsyncStatus32::Complete { .. }))
    }
}

impl AsyncRuntime32 {
    pub(in crate::process::windows::files) fn cancel_open(&mut self, open: OpenFileId) {
        self.cancel_description(crate::windows::files::completion::CompletionTarget32::File(
            open,
        ));
    }
    pub(in crate::process::windows::files) fn cancel_description(
        &mut self,
        target: crate::windows::files::completion::CompletionTarget32,
    ) {
        for operation in &mut self.operations {
            if operation.target.description() == target
                && operation.status == AsyncStatus32::Pending
            {
                operation.cancel = true;
            }
        }
    }
}

impl AsyncRuntime32 {
    pub(in crate::process::windows::files) fn process_exit_bindings(
        &self,
    ) -> Result<Vec<EventBinding32>, FileError32> {
        let count = self
            .operations
            .iter()
            .try_fold(0usize, |count, operation| {
                count
                    .checked_add(operation.events.len())
                    .ok_or(FileError32::Capacity)
            })?;
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for operation in &self.operations {
            bindings.extend_from_slice(&operation.events);
        }
        Ok(bindings)
    }
    pub(in crate::process::windows::files) fn commit_process_exit(&mut self) {
        self.operations.clear();
        self.retained_bytes = 0;
        self.wait = None;
        self.interrupted = false;
        self.blocking = None;
    }
    pub(in crate::process::windows::files) fn cancel_wait(&mut self) {
        self.wait = None;
        self.interrupted = false;
    }
}
