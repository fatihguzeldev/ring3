use super::*;

impl Vfs32 {
    pub(crate) fn prepare_delete(&self, input: &[u16]) -> Result<PreparedNamespace32, FileError32> {
        let file = self.lookup(&self.full_path(input)?)?;
        if self.node(file)?.directory() {
            return Err(FileError32::AccessDenied);
        }
        self.prepare_delete_node(file)
    }
    pub(super) fn prepare_delete_node(
        &self,
        file: FileId,
    ) -> Result<PreparedNamespace32, FileError32> {
        let node = self.node(file)?;
        if (node.attributes & ATTR_READONLY != 0 && !node.directory()) || node.delete_pending {
            return Err(FileError32::AccessDenied);
        }
        self.check_sharing(file, FileAccess32::DELETE, FileShare32::ALL)?;
        if self.deleted.len() >= self.limits.nodes {
            return Err(FileError32::Capacity);
        }
        self.namespace_receipt(Operation32::Delete {
            file,
            immediate: node.descriptions == 0,
        })
    }
}

impl Vfs32 {
    pub(crate) fn prepare_cancel_copy_destination(
        &self,
        file: FileId,
    ) -> Result<PreparedNamespace32, FileError32> {
        let node = self.node(file)?;
        if node.directory() {
            return Err(FileError32::AccessDenied);
        }
        if node.linked && node.descriptions == 0 && self.deleted.len() >= self.limits.nodes {
            return Err(FileError32::Capacity);
        }
        self.namespace_receipt(Operation32::Delete {
            file,
            immediate: node.descriptions == 0,
        })
    }
}
