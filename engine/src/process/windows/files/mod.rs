mod asynchronous;
mod calls;
mod handles;
mod host;
mod lifecycle;
mod mapping;
mod memory;
mod ordinary;
mod process;
mod security;
mod waits;

use crate::{
    files::{FileError32, FileLimits32, OpenFileId, Vfs32},
    memory::{GuestAddress, PageRange},
    process::calls::call::PendingOwner,
    windows::CallFrame32,
};
use asynchronous::AsyncRuntime32;
pub(in crate::process) use asynchronous::FileThreadReply32;
use handles::KernelFileHandles32;
pub use host::{FileInput32, FileMountStart32};
use mapping::{MappingLimits32, MappingRuntime32};
pub(in crate::process) use ordinary::FileCallOutcome32;
pub use ordinary::FileEnvironment32;
use ordinary::OrdinaryRuntime32;

pub use crate::files::{
    AssetRef32, FileId, FileInfo32, FileReadRequest32, FileTimes32, OverlayEntry32, OverlayKind32,
};
pub use crate::files::{FileError32 as WindowsFileError32, FileLimits32 as VfsLimits32};

#[derive(Clone, Copy, Debug)]
pub struct FileConfig32 {
    pub vfs: FileLimits32,
    pub handles: u32,
    pub finds: usize,
    pub mappings: usize,
    pub views: usize,
    pub view_bounds: PageRange,
    pub operations: usize,
    pub operation_bytes: usize,
}
impl Default for FileConfig32 {
    fn default() -> Self {
        Self {
            vfs: FileLimits32::default(),
            handles: 4096,
            finds: 256,
            mappings: 256,
            views: 1024,
            view_bounds: PageRange::new(GuestAddress(0x1000_0000), 0x60000).unwrap(),
            operations: 128,
            operation_bytes: 16 * 1024 * 1024,
        }
    }
}

pub(in crate::process) struct FileRuntime32 {
    vfs: Vfs32,
    handles: KernelFileHandles32,
    ordinary: OrdinaryRuntime32,
    mappings: MappingRuntime32,
    asynchronous: AsyncRuntime32,
    reply: Option<FileReply32>,
    events: Vec<(OpenFileId, u32)>,
    host_input: host::HostFileInput32,
}
#[derive(Clone, Copy)]
struct FileReply32 {
    owner: PendingOwner,
    token: u32,
    frame: CallFrame32,
    id: u32,
    request: Option<FileReadRequest32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileWait32 {
    pub epoch: u64,
    pub token: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileProgress32 {
    pub operation: u64,
    pub error: u32,
    pub bytes: u32,
    pub pending_bytes: bool,
}

#[cfg(test)]
mod tests;
