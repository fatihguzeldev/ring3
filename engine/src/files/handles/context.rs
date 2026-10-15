use super::*;

impl Vfs32 {
    pub(crate) fn file_for_open(&self, open: OpenFileId) -> Result<FileId, FileError32> {
        Ok(self.open(open)?.file)
    }
    pub(crate) fn open_info(&self, open: OpenFileId) -> Result<FileInfo32, FileError32> {
        self.info(self.file_for_open(open)?)
    }
    pub(crate) fn open_access(&self, open: OpenFileId) -> Result<FileAccess32, FileError32> {
        Ok(self.open(open)?.options.access)
    }
    pub(crate) fn open_options(&self, open: OpenFileId) -> Result<FileOpenOptions32, FileError32> {
        Ok(self.open(open)?.options)
    }
    pub(crate) fn opened_path(&self, open: OpenFileId) -> Result<Vec<u16>, FileError32> {
        Ok(self.open(open)?.opened_path.clone())
    }
    pub(crate) fn position(&self, open: OpenFileId) -> Result<u64, FileError32> {
        Ok(self.open(open)?.position)
    }
    pub(in crate::files) fn open(&self, id: OpenFileId) -> Result<&Open32, FileError32> {
        self.opens
            .iter()
            .flatten()
            .find(|o| o.id == id)
            .ok_or(FileError32::InvalidHandle)
    }
    pub(in crate::files) fn open_mut(
        &mut self,
        id: OpenFileId,
    ) -> Result<&mut Open32, FileError32> {
        self.opens
            .iter_mut()
            .flatten()
            .find(|o| o.id == id)
            .ok_or(FileError32::InvalidHandle)
    }
    pub(in crate::files) fn check_sharing(
        &self,
        file: FileId,
        access: FileAccess32,
        share: FileShare32,
    ) -> Result<(), FileError32> {
        for open in self.opens.iter().flatten().filter(|o| o.file == file) {
            if access_mask(access) & !open.options.share.0 != 0
                || access_mask(open.options.access) & !share.0 != 0
            {
                return Err(FileError32::SharingViolation);
            }
        }
        Ok(())
    }
}

fn access_mask(access: FileAccess32) -> u32 {
    (if access.0 & (FileAccess32::READ.0 | FileAccess32::EXECUTE.0) != 0 {
        FileShare32::READ.0
    } else {
        0
    }) | if access.0 & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0) != 0 {
        FileShare32::WRITE.0
    } else {
        0
    } | if access.contains(FileAccess32::DELETE) {
        FileShare32::DELETE.0
    } else {
        0
    }
}
