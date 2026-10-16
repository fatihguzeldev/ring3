mod closing;
mod context;
mod lifetime;
mod opening;
mod termination;

pub(crate) use termination::PreparedFileExit32;

use super::{
    path,
    tree::{Content32, NamespaceEntry32, Node32},
    *,
};

#[derive(Clone, Debug)]
pub(super) struct Open32 {
    pub id: OpenFileId,
    pub file: FileId,
    pub references: u32,
    pub position: u64,
    pub cursor_revision: u64,
    pub options: FileOpenOptions32,
    pub suppress_access: bool,
    pub suppress_write: bool,
    pub opened_path: Vec<u16>,
    pub maximum_allowed: bool,
}
#[derive(Debug)]
pub(crate) struct PreparedOpen32 {
    epoch: u64,
    revision: u64,
    next: u64,
    result: FileOpenResult32,
    open: Open32,
    created: Option<(Node32, NamespaceEntry32)>,
    truncate: bool,
    content_revision: u64,
    granted_access: u32,
}
impl PreparedOpen32 {
    pub(crate) fn granted_access(&self) -> u32 {
        self.granted_access
    }
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
