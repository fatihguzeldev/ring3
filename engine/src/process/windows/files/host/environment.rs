use super::*;

impl EngineInstance {
    fn check_file_environment_change(&self) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::File(FileError32::Busy));
        }
        Ok(())
    }
    pub fn set_files_environment(
        &mut self,
        epoch: u64,
        environment: FileEnvironment32,
    ) -> Result<(), HostError> {
        self.check_file_environment_change()?;
        let files = self.files_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        if files.host_input.pending.is_some() {
            return Err(HostError::File(FileError32::Busy));
        }
        files
            .set_file_environment(environment)
            .map_err(HostError::File)
    }
    pub fn begin_file_environment(
        &mut self,
        epoch: u64,
        lengths: &[u32],
    ) -> Result<FileInput32, HostError> {
        let files = self.files_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        if !(4..=260).contains(&lengths.len())
            || lengths.iter().any(|length| !(3..=32767).contains(length))
        {
            return Err(HostError::File(FileError32::InvalidParameter));
        }
        let total = lengths
            .iter()
            .try_fold(0u32, |total, length| total.checked_add(*length))
            .ok_or(HostError::File(FileError32::Capacity))?;
        if total > 524288 {
            return Err(HostError::File(FileError32::Capacity));
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(lengths.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        owned.extend_from_slice(lengths);
        files
            .host_input
            .begin(epoch, total, FileInputKind32::Environment(owned))
            .map_err(HostError::File)
    }
    pub fn finish_file_environment(&mut self, epoch: u64, token: u64) -> Result<(), HostError> {
        self.check_file_environment_change()?;
        let files = self.files_mut()?;
        let pending = files
            .host_input
            .checked(epoch, token)
            .map_err(HostError::File)?;
        if pending.units.len() != pending.total as usize {
            return Err(HostError::File(FileError32::Busy));
        }
        let FileInputKind32::Environment(lengths) = &pending.kind else {
            return Err(HostError::File(FileError32::InvalidParameter));
        };
        let mut paths = Vec::new();
        paths
            .try_reserve_exact(lengths.len())
            .map_err(|_| HostError::File(FileError32::Allocation))?;
        let mut offset = 0;
        for &length in lengths {
            let end = offset + length as usize;
            let mut path = Vec::new();
            path.try_reserve_exact(length as usize)
                .map_err(|_| HostError::File(FileError32::Allocation))?;
            path.extend_from_slice(&pending.units[offset..end]);
            paths.push(path);
            offset = end;
        }
        let application_directory = paths.remove(0);
        let windows_directory = paths.remove(0);
        let system_directory = paths.remove(0);
        let temp_directory = paths.remove(0);
        let environment = FileEnvironment32 {
            application_directory,
            windows_directory,
            system_directory,
            temp_directory,
            search_path: paths,
        };
        files
            .set_file_environment(environment)
            .map_err(HostError::File)?;
        files.host_input.pending = None;
        Ok(())
    }
}
