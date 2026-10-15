mod deletion;
mod directories;
mod metadata;
mod publication;
mod replacement;
mod transfers;

use super::{
    path,
    tree::{Content32, NamespaceEntry32, Node32},
    *,
};

#[derive(Debug)]
pub(crate) struct PreparedNamespace32 {
    epoch: u64,
    revision: u64,
    next: u64,
    operation: Operation32,
}
#[derive(Debug)]
enum Operation32 {
    Create(Node32, NamespaceEntry32),
    Delete {
        file: FileId,
        immediate: bool,
    },
    Move {
        updates: Vec<NamespaceEntry32>,
        old: NamespaceEntry32,
        replaced: Option<FileId>,
    },
    Copy {
        node: Node32,
        entry: Option<NamespaceEntry32>,
    },
    Replace {
        replaced: FileId,
        replacement: FileId,
        target: NamespaceEntry32,
        backup: Option<NamespaceEntry32>,
        discard_backup: Option<FileId>,
        old: NamespaceEntry32,
        created: u64,
    },
    Attributes {
        file: FileId,
        attributes: u32,
    },
    Times {
        file: FileId,
        created: Option<u64>,
        accessed: Option<u64>,
        written: Option<u64>,
        open: (OpenFileId, bool, bool),
    },
    Directory {
        directory: Vec<u16>,
        drive_copy: Option<Vec<u16>>,
    },
}
impl PreparedNamespace32 {
    pub(crate) fn validate(&self, vfs: &Vfs32) -> Result<(), FileError32> {
        vfs.validate(self.epoch, self.revision)
    }
}
impl Vfs32 {
    fn namespace_receipt(
        &self,
        operation: Operation32,
    ) -> Result<PreparedNamespace32, FileError32> {
        let (revision, next) = self.stamp()?;
        Ok(PreparedNamespace32 {
            epoch: self.epoch,
            revision,
            next,
            operation,
        })
    }
}
