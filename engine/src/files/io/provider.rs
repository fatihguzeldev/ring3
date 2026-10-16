use super::*;

impl Vfs32 {
    pub(crate) fn read_provider_file(
        &mut self,
        open: OpenFileId,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<Vec<u8>>, FileError32> {
        let description = self.open(open)?;
        if !description.options.access.contains(FileAccess32::READ) {
            return Err(FileError32::AccessDenied);
        }
        let file = description.file;
        match self.read_at_owned(FileIoOwner32::ProviderFile(open), file, offset, length)? {
            FileIo32::NeedBytes(request) => Ok(FileIo32::NeedBytes(request)),
            FileIo32::Ready(read) => {
                read.validate(self)?;
                Ok(FileIo32::Ready(read.bytes))
            }
        }
    }

    pub(crate) fn cancel_provider_file_io(&mut self, open: OpenFileId) {
        self.cancel_owned_io(FileIoOwner32::ProviderFile(open));
    }
}
