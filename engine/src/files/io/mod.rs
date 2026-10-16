mod allocation;
mod positioning;
mod provider;
mod reading;
mod requests;
mod writing;

use super::{tree::OwnedPage32, *};

#[derive(Debug)]
pub(crate) struct PreparedRead32 {
    epoch: u64,
    revision: u64,
    next: u64,
    bytes: Vec<u8>,
    cursor: Option<(OpenFileId, u64)>,
}
impl PreparedRead32 {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
#[derive(Debug)]
pub(crate) struct PreparedWrite32 {
    epoch: u64,
    revision: u64,
    next: u64,
    file: FileId,
    pages: Vec<OwnedPage32>,
    size: u64,
    count: usize,
    content_revision: u64,
    cursor: Option<(OpenFileId, u64)>,
}
impl PreparedWrite32 {
    pub(crate) fn length(&self) -> usize {
        self.count
    }
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
#[derive(Debug)]
pub(crate) struct PreparedResize32 {
    epoch: u64,
    revision: u64,
    next: u64,
    file: FileId,
    size: u64,
    content_revision: u64,
    open: Option<OpenFileId>,
}
impl PreparedResize32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
#[derive(Debug)]
pub(crate) struct PreparedSeek32 {
    epoch: u64,
    revision: u64,
    next: u64,
    open: OpenFileId,
    position: u64,
}
impl PreparedSeek32 {
    pub(crate) fn position(&self) -> u64 {
        self.position
    }
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
pub(super) struct WriteStage32 {
    pub owner: FileIoOwner32,
    pub file: FileId,
    pub revision: u64,
    pub spans: Vec<(u64, Vec<u8>)>,
    pub offsets: Vec<u64>,
    pub pages: Vec<OwnedPage32>,
    pub waiting: Option<FileReadRequest32>,
}
