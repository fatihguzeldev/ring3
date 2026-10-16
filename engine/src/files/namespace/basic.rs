use super::*;

impl Vfs32 {
    pub(crate) fn prepare_basic_information(
        &self,
        open: OpenFileId,
        times: [u64; 4],
        attributes: u32,
    ) -> Result<PreparedNamespace32, FileError32> {
        let description = self.open(open)?;
        if !description
            .options
            .access
            .contains(FileAccess32::WRITE_ATTRIBUTES)
        {
            return Err(FileError32::AccessDenied);
        }
        let node = self.node(description.file)?;
        if attributes & !0x31b7 != 0 {
            return Err(FileError32::Unsupported);
        }
        if times
            .iter()
            .any(|&time| time > i64::MAX as u64 && time < u64::MAX - 1)
        {
            return Err(FileError32::InvalidParameter);
        }
        let suppress = |old, time| match time {
            u64::MAX => true,
            value if value == u64::MAX - 1 => false,
            _ => old,
        };
        let suppress = [
            suppress(description.suppress_access, times[1]),
            suppress(description.suppress_write, times[2]),
            suppress(description.suppress_change, times[3]),
        ];
        let attributes = if attributes == 0 {
            None
        } else {
            let attributes = (attributes & !ATTR_DIRECTORY) | (node.attributes & ATTR_DIRECTORY);
            Some(if attributes & !ATTR_NORMAL == 0 {
                ATTR_NORMAL
            } else {
                attributes & !ATTR_NORMAL
            })
        };
        self.namespace_receipt(Operation32::Basic {
            file: description.file,
            open,
            times,
            attributes,
            suppress,
        })
    }
}
