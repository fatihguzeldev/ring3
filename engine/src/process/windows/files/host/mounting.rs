use super::*;

impl EngineInstance {
    pub fn mount_file(
        &mut self,
        path: &[u16],
        asset: AssetRef32,
        attributes: u32,
        times: FileTimes32,
    ) -> Result<FileId, HostError> {
        self.files_mut()?
            .vfs
            .mount_asset(path, asset, attributes, times)
            .map_err(HostError::File)
    }
    pub fn mount_directory(
        &mut self,
        path: &[u16],
        attributes: u32,
        times: FileTimes32,
    ) -> Result<FileId, HostError> {
        self.files_mut()?
            .vfs
            .mount_directory(path, attributes, times)
            .map_err(HostError::File)
    }
    pub fn set_files_time(&mut self, epoch: u64, now_filetime: u64) -> Result<(), HostError> {
        let files = self.files_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        files.vfs.set_time(now_filetime).map_err(HostError::File)
    }
}

impl EngineInstance {
    pub fn begin_file_mount(
        &mut self,
        epoch: u64,
        start: FileMountStart32,
    ) -> Result<FileInput32, HostError> {
        let files = self.files_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        if start.units == 0
            || start.units > 32767
            || start
                .asset
                .is_some_and(|a| a.id == 0 || start.attributes & 0x10 != 0)
        {
            return Err(HostError::File(FileError32::InvalidParameter));
        }
        files
            .host_input
            .begin(epoch, start.units, FileInputKind32::Mount(start))
            .map_err(HostError::File)
    }
    pub fn finish_file_mount(&mut self, epoch: u64, token: u64) -> Result<FileId, HostError> {
        let files = self.files_mut()?;
        let pending = files
            .host_input
            .checked(epoch, token)
            .map_err(HostError::File)?;
        if pending.units.len() != pending.total as usize {
            return Err(HostError::File(FileError32::Busy));
        }
        let FileInputKind32::Mount(start) = pending.kind else {
            return Err(HostError::File(FileError32::InvalidParameter));
        };
        let file = if let Some(asset) = start.asset {
            files
                .vfs
                .mount_asset(&pending.units, asset, start.attributes, start.times)
        } else {
            files
                .vfs
                .mount_directory(&pending.units, start.attributes, start.times)
        }
        .map_err(HostError::File)?;
        files.host_input.pending = None;
        Ok(file)
    }
}
