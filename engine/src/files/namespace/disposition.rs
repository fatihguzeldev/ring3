use super::*;

impl Vfs32 {
    pub(crate) fn prepare_disposition(
        &self,
        open: OpenFileId,
        delete: bool,
    ) -> Result<PreparedNamespace32, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::DELETE) {
            return Err(FileError32::AccessDenied);
        }
        let node = self.node(description.file)?;
        if delete && !description.options.delete_on_close {
            if self.has_file_section(description.file) || node.attributes & ATTR_READONLY != 0 {
                return Err(FileError32::AccessDenied);
            }
            let path = self
                .entries
                .iter()
                .find(|entry| entry.file == description.file)
                .ok_or(FileError32::AccessDenied)?;
            if path::root_len(&path.path)? == path.path.len() {
                return Err(FileError32::AccessDenied);
            }
            if node.directory() && !self.directory_empty(description.file)? {
                return Err(FileError32::DirectoryNotEmpty);
            }
            self.check_sharing_except(
                description.file,
                FileAccess32::DELETE,
                FileShare32::ALL,
                Some(open),
            )?;
        }
        self.namespace_receipt(Operation32::Disposition {
            open,
            delete: delete && !description.options.delete_on_close,
        })
    }
}
