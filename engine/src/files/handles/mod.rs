mod closing;
mod context;
mod lifetime;
mod opening;

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
}
impl PreparedOpen32 {
    pub(crate) fn result(&self) -> FileOpenResult32 {
        self.result
    }
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
