use super::*;

impl EngineInstance {
    pub fn file_overlay_entry(
        &self,
        epoch: u64,
        index: usize,
    ) -> Result<Option<OverlayEntry32>, HostError> {
        let files = self.files_storage()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        files.vfs.overlay_entry(index).map_err(HostError::File)
    }
    pub fn acknowledge_file_overlay(&mut self, epoch: u64, revision: u64) -> Result<(), HostError> {
        self.files_storage_mut()?
            .vfs
            .acknowledge_overlay(epoch, revision)
            .map_err(HostError::File)
    }
    pub fn read_file_overlay(
        &mut self,
        epoch: u64,
        file: FileId,
        revision: u64,
        offset: u64,
        output: &mut [u8],
    ) -> Result<Option<usize>, HostError> {
        let files = self.files_storage_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        match files
            .vfs
            .read_overlay(file, revision, offset, output.len())
            .map_err(HostError::File)?
        {
            FileIo32::Ready(read) => {
                let bytes = read.bytes();
                output[..bytes.len()].copy_from_slice(bytes);
                Ok(Some(bytes.len()))
            }
            FileIo32::NeedBytes(_) => Ok(None),
        }
    }
}

impl EngineInstance {
    pub fn read_file_overlay_path(
        &self,
        epoch: u64,
        revision: u64,
        index: usize,
        offset: u32,
        output: &mut [u16],
    ) -> Result<usize, HostError> {
        let files = self.files_storage()?;
        if files.vfs.epoch() != epoch || files.vfs.revision() != revision {
            return Err(HostError::File(FileError32::Stale));
        }
        let entry = files
            .vfs
            .overlay_entry(index)
            .map_err(HostError::File)?
            .ok_or(HostError::File(FileError32::NoMoreFiles))?;
        let offset = offset as usize;
        if offset > entry.path.len() {
            return Err(HostError::File(FileError32::InvalidParameter));
        }
        let count = (entry.path.len() - offset).min(output.len());
        output[..count].copy_from_slice(&entry.path[offset..offset + count]);
        Ok(count)
    }
}
