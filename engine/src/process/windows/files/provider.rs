use super::*;
use crate::{
    files::{FileAccess32, FileDisposition32, FileIo32, FileOpenOptions32, FileShare32},
    process::{EngineInstance, HostError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct ProviderFile32 {
    epoch: u64,
    open: OpenFileId,
    thread: u32,
}

pub(super) struct ProviderFiles32 {
    leases: Vec<ProviderFile32>,
    limit: usize,
}

impl ProviderFiles32 {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            leases: Vec::new(),
            limit,
        }
    }
    pub(super) fn thread_opens(&self, thread: u32) -> impl Iterator<Item = OpenFileId> + '_ {
        self.leases
            .iter()
            .filter(move |lease| lease.thread == thread)
            .map(|lease| lease.open)
    }
    pub(super) fn clear(&mut self) {
        self.leases.clear();
    }
}

impl FileRuntime32 {
    fn check_provider_file(&self, lease: ProviderFile32, thread: u32) -> Result<(), FileError32> {
        if lease.epoch != self.vfs.epoch() || lease.thread != thread {
            return Err(FileError32::Stale);
        }
        if !self.provider.leases.contains(&lease) {
            return Err(FileError32::InvalidHandle);
        }
        self.vfs.open_info(lease.open)?;
        Ok(())
    }
    fn open_provider_file(
        &mut self,
        path: &[u16],
        write: bool,
        thread: u32,
        token: &crate::windows::security::Token32,
    ) -> Result<ProviderFile32, FileError32> {
        if self.provider.leases.len() >= self.provider.limit {
            return Err(FileError32::Capacity);
        }
        self.provider
            .leases
            .try_reserve(1)
            .map_err(|_| FileError32::Allocation)?;
        if write
            && let Ok(info) = self.vfs.path_info(path)
            && self.mappings.minimum_file_size(info.id) > 0
        {
            return Err(FileError32::UserMappedFile);
        }
        let options = FileOpenOptions32 {
            access: if write {
                FileAccess32::WRITE
            } else {
                FileAccess32::READ
            },
            share: FileShare32::READ,
            disposition: if write {
                FileDisposition32::CreateAlways
            } else {
                FileDisposition32::OpenExisting
            },
            ..FileOpenOptions32::default()
        };
        let desired = if write { 0x4000_0000 } else { 0x8000_0000 };
        let prepared = self
            .vfs
            .prepare_open_with_security(path, options, desired, token, None)?;
        prepared.validate(&self.vfs)?;
        let result = self.vfs.commit_open(prepared);
        let lease = ProviderFile32 {
            epoch: self.vfs.epoch(),
            open: result.open,
            thread,
        };
        self.provider.leases.push(lease);
        Ok(lease)
    }
    pub(super) fn retire_provider_thread_files(&mut self, thread: u32) {
        for lease in self
            .provider
            .leases
            .iter()
            .filter(|lease| lease.thread == thread)
        {
            self.vfs.cancel_provider_file_io(lease.open);
            self.vfs
                .close_open(lease.open)
                .expect("provider file closes were preflighted with thread exit");
        }
        self.provider.leases.retain(|lease| lease.thread != thread);
    }
}

impl EngineInstance {
    pub(in crate::process) fn find_provider_paths(
        &self,
        pattern: &[u16],
        limit: usize,
    ) -> Result<Vec<Vec<u16>>, HostError> {
        if limit == 0 || limit > 4096 {
            return Err(HostError::File(FileError32::InvalidParameter));
        }
        let files = self.files_storage()?;
        let entries = files
            .vfs
            .find_with_security(pattern, limit, self.windows_security.token())
            .map_err(HostError::File)?;
        let mut paths = Vec::new();
        paths
            .try_reserve_exact(entries.len())
            .map_err(|_| HostError::Infrastructure)?;
        for entry in entries {
            paths.push(
                files
                    .vfs
                    .file_path(entry.info.id)
                    .map_err(HostError::File)?,
            );
        }
        Ok(paths)
    }
    pub(in crate::process) fn open_provider_file(
        &mut self,
        path: &[u16],
        write: bool,
    ) -> Result<ProviderFile32, HostError> {
        let thread = self.current_windows_thread_id()?;
        self.memory()?;
        self.files
            .as_mut()
            .ok_or(HostError::File(FileError32::Unsupported))?
            .open_provider_file(path, write, thread, self.windows_security.token())
            .map_err(HostError::File)
    }
    pub(in crate::process) fn provider_file_size(
        &self,
        lease: ProviderFile32,
    ) -> Result<u64, HostError> {
        let thread = self.current_windows_thread_id()?;
        let files = self.files_storage()?;
        files
            .check_provider_file(lease, thread)
            .map_err(HostError::File)?;
        Ok(files
            .vfs
            .open_info(lease.open)
            .map_err(HostError::File)?
            .size)
    }
    pub(in crate::process) fn provider_file_info(
        &self,
        lease: ProviderFile32,
    ) -> Result<FileInfo32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let files = self.files_storage()?;
        files
            .check_provider_file(lease, thread)
            .map_err(HostError::File)?;
        files.vfs.open_info(lease.open).map_err(HostError::File)
    }
    pub(in crate::process) fn provider_file_path(
        &self,
        lease: ProviderFile32,
    ) -> Result<Vec<u16>, HostError> {
        let thread = self.current_windows_thread_id()?;
        let files = self.files_storage()?;
        files
            .check_provider_file(lease, thread)
            .map_err(HostError::File)?;
        files.vfs.opened_path(lease.open).map_err(HostError::File)
    }
    pub(in crate::process) fn check_close_provider_files(
        &self,
        leases: &[ProviderFile32],
    ) -> Result<(), HostError> {
        if leases.is_empty() {
            return Ok(());
        }
        let thread = self.current_windows_thread_id()?;
        let files = self.files_storage()?;
        if leases.len() > files.provider.limit {
            return Err(HostError::File(FileError32::InvalidParameter));
        }
        let mut closes = Vec::new();
        closes
            .try_reserve_exact(leases.len())
            .map_err(|_| HostError::Infrastructure)?;
        for (index, &lease) in leases.iter().enumerate() {
            if leases[..index].contains(&lease) {
                return Err(HostError::File(FileError32::InvalidParameter));
            }
            files
                .check_provider_file(lease, thread)
                .map_err(HostError::File)?;
            closes.push((lease.open, 1));
        }
        files
            .vfs
            .check_close_open_group(&closes, 0, &[])
            .map_err(HostError::File)
    }
    pub(in crate::process) fn commit_close_provider_files(&mut self, leases: &[ProviderFile32]) {
        if leases.is_empty() {
            return;
        }
        let files = self
            .files
            .as_mut()
            .expect("provider close group has a live canonical filesystem");
        for lease in leases {
            files
                .vfs
                .close_open(lease.open)
                .expect("provider close group was preflighted in the same transaction");
            files.vfs.cancel_provider_file_io(lease.open);
        }
        files
            .provider
            .leases
            .retain(|lease| !leases.contains(lease));
    }
    pub(in crate::process) fn read_provider_file(
        &mut self,
        lease: ProviderFile32,
        offset: u64,
        length: usize,
    ) -> Result<FileIo32<Vec<u8>>, HostError> {
        let thread = self.current_windows_thread_id()?;
        let files = self.files_mut()?;
        files
            .check_provider_file(lease, thread)
            .map_err(HostError::File)?;
        files
            .vfs
            .read_provider_file(lease.open, offset, length)
            .map_err(HostError::File)
    }
    pub(in crate::process) fn provider_file_request_ready(
        &self,
        request: FileReadRequest32,
    ) -> bool {
        self.files
            .as_ref()
            .is_some_and(|files| files.vfs.request_ready(request))
    }
}
