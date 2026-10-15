use super::*;

impl Vfs32 {
    pub(crate) fn prepare_set_attributes(
        &self,
        input: &[u16],
        attributes: u32,
    ) -> Result<PreparedNamespace32, FileError32> {
        if attributes & !0x31b7 != 0 {
            return Err(FileError32::Unsupported);
        }
        let file = self.lookup(&self.full_path(input)?)?;
        let node = self.node(file)?;
        if node.delete_pending {
            return Err(FileError32::AccessDenied);
        }
        let attributes =
            (attributes & !ATTR_DIRECTORY) | if node.directory() { ATTR_DIRECTORY } else { 0 };
        self.namespace_receipt(Operation32::Attributes { file, attributes })
    }
    pub(crate) fn prepare_file_times(
        &self,
        open: OpenFileId,
        created: Option<u64>,
        accessed: Option<u64>,
        written: Option<u64>,
    ) -> Result<PreparedNamespace32, FileError32> {
        let description = self.open(open)?;
        if !description
            .options
            .access
            .contains(FileAccess32::WRITE_ATTRIBUTES)
        {
            return Err(FileError32::AccessDenied);
        }
        if created == Some(u64::MAX) {
            return Err(FileError32::InvalidParameter);
        }
        self.namespace_receipt(Operation32::Times {
            file: description.file,
            created,
            accessed: accessed.filter(|n| *n != u64::MAX),
            written: written.filter(|n| *n != u64::MAX),
            open: (
                open,
                accessed == Some(u64::MAX) || description.suppress_access,
                written == Some(u64::MAX) || description.suppress_write,
            ),
        })
    }
}
