use super::locks::ByteRangeLock32;
use crate::files::{DirectoryEntry32, FileError32};

pub(super) struct Find32 {
    pub id: u64,
    pub entries: Vec<DirectoryEntry32>,
    pub cursor: usize,
}

pub(in crate::process::windows::files) struct OrdinaryRuntime32 {
    pub(in crate::process::windows::files) ansi: crate::windows::text::CodePage32,
    pub(super) finds: Vec<Find32>,
    pub(super) find_limit: usize,
    pub(super) sequence: u64,
    pub(super) error_mode: u32,
    pub(super) thread_error_mode: u32,
    pub(super) temp_sequence: u16,
    pub(super) environment: super::FileEnvironment32,
    pub(super) locks: Vec<ByteRangeLock32>,
    pub(super) lock_limit: usize,
    pub(super) directories: Vec<super::handles::information::DirectoryQuery32>,
    pub(super) copies: Vec<super::progress::PendingCopy32>,
}

impl OrdinaryRuntime32 {
    pub(in crate::process::windows::files) fn terminate(&mut self) {
        self.finds.clear();
        self.locks.clear();
        self.copies.clear();
        self.directories.clear();
    }

    pub(in crate::process::windows::files) fn new(
        max_finds: usize,
        max_locks: usize,
    ) -> Result<Self, FileError32> {
        if max_finds == 0 || max_finds > 65_536 || max_locks == 0 || max_locks > 65_536 {
            return Err(FileError32::InvalidLimits);
        }
        let mut finds = Vec::new();
        finds
            .try_reserve_exact(max_finds)
            .map_err(|_| FileError32::Allocation)?;
        let mut locks = Vec::new();
        locks
            .try_reserve_exact(max_locks)
            .map_err(|_| FileError32::Allocation)?;
        let mut copies = Vec::new();
        copies
            .try_reserve_exact(33)
            .map_err(|_| FileError32::Allocation)?;
        let mut directories = Vec::new();
        directories
            .try_reserve_exact(max_finds)
            .map_err(|_| FileError32::Allocation)?;
        Ok(Self {
            ansi: Default::default(),
            finds,
            find_limit: max_finds,
            sequence: 0,
            error_mode: 0,
            thread_error_mode: 0,
            temp_sequence: 0,
            environment: super::FileEnvironment32::default(),
            locks,
            lock_limit: max_locks,
            copies,
            directories,
        })
    }
}
