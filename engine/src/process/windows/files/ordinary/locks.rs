use super::FileCallOutcome32;
use crate::{
    files::{FileAccess32, FileError32, FileId, OpenFileId},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

#[derive(Clone, Copy)]
pub(super) struct ByteRangeLock32 {
    file: FileId,
    owner: OpenFileId,
    start: u64,
    end: u64,
    exclusive: bool,
}
pub(in crate::process::windows::files) struct PreparedFileLock32 {
    entry: ByteRangeLock32,
}

fn range(start: u64, length: u64) -> Result<u64, FileError32> {
    if length == 0 {
        return Err(FileError32::InvalidParameter);
    }
    start
        .checked_add(length)
        .ok_or(FileError32::InvalidParameter)
}
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn check_file_lock(
        &self,
        open: OpenFileId,
        offset: u64,
        length: u64,
        write: bool,
    ) -> Result<(), FileError32> {
        let file = self.vfs.file_for_open(open)?;
        if length == 0 {
            return Ok(());
        }
        let end = range(offset, length)?;
        if self.ordinary.locks.iter().any(|lock| {
            lock.file == file
                && offset < lock.end
                && lock.start < end
                && if lock.exclusive {
                    lock.owner != open
                } else {
                    write
                }
        }) {
            return Err(FileError32::LockViolation);
        }
        Ok(())
    }
    pub(in crate::process::windows::files::ordinary) fn check_copy_file_lock(
        &self,
        file: FileId,
        length: u64,
        write: bool,
    ) -> Result<(), FileError32> {
        if self
            .ordinary
            .locks
            .iter()
            .any(|lock| lock.file == file && lock.start < length && (write || lock.exclusive))
        {
            return Err(FileError32::LockViolation);
        }
        Ok(())
    }
    pub(in crate::process::windows::files) fn prepare_file_lock(
        &self,
        open: OpenFileId,
        offset: u64,
        length: u64,
        exclusive: bool,
    ) -> Result<PreparedFileLock32, FileError32> {
        let file = self.vfs.file_for_open(open)?;
        let end = range(offset, length)?;
        if self.ordinary.locks.iter().any(|lock| {
            lock.file == file
                && offset < lock.end
                && lock.start < end
                && (exclusive || lock.exclusive && lock.owner != open)
        }) {
            return Err(FileError32::LockViolation);
        }
        if self.ordinary.locks.len() == self.ordinary.lock_limit {
            return Err(FileError32::Capacity);
        }
        Ok(PreparedFileLock32 {
            entry: ByteRangeLock32 {
                file,
                owner: open,
                start: offset,
                end,
                exclusive,
            },
        })
    }
    pub(in crate::process::windows::files) fn commit_file_lock(
        &mut self,
        prepared: PreparedFileLock32,
    ) {
        assert!(self.ordinary.locks.len() < self.ordinary.lock_limit);
        self.ordinary.locks.push(prepared.entry);
    }
    pub(in crate::process::windows::files) fn unlock_file_range(
        &mut self,
        open: OpenFileId,
        offset: u64,
        length: u64,
    ) -> Result<(), FileError32> {
        let index = self.unlock_file_index(open, offset, length)?;
        self.ordinary.locks.remove(index);
        Ok(())
    }
    pub(in crate::process::windows::files) fn check_unlock_file_range(
        &self,
        open: OpenFileId,
        offset: u64,
        length: u64,
    ) -> Result<(), FileError32> {
        self.unlock_file_index(open, offset, length)?;
        Ok(())
    }
    fn unlock_file_index(
        &self,
        open: OpenFileId,
        offset: u64,
        length: u64,
    ) -> Result<usize, FileError32> {
        self.vfs.file_for_open(open)?;
        let end = range(offset, length)?;
        let matches =
            |lock: &ByteRangeLock32| lock.owner == open && lock.start == offset && lock.end == end;
        self.ordinary
            .locks
            .iter()
            .position(|lock| matches(lock) && lock.exclusive)
            .or_else(|| self.ordinary.locks.iter().rposition(matches))
            .ok_or(FileError32::LockViolation)
    }
    pub(in crate::process::windows::files) fn release_file_locks(&mut self, open: OpenFileId) {
        self.ordinary.locks.retain(|lock| lock.owner != open);
        self.ordinary.directories.retain(|query| query.open != open);
    }
    pub(super) fn lock_file_call(
        &mut self,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        if self.handles.access(args[0])?
            & (FileAccess32::READ.0 | FileAccess32::WRITE.0 | FileAccess32::APPEND.0)
            == 0
        {
            return Err(FileError32::AccessDenied);
        }
        let offset = u64::from(args[1]) | u64::from(args[2]) << 32;
        let length = u64::from(args[3]) | u64::from(args[4]) << 32;
        if api == FileApi32::LockFile {
            let prepared = self.prepare_file_lock(open, offset, length, true)?;
            self.commit_file_lock(prepared);
        } else {
            self.unlock_file_range(open, offset, length)?;
        }
        Ok(FileCallOutcome32::success(1))
    }
}
