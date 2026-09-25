use super::{Directory, DispatchError, LoadError, Process32};

const MAX_BYTES: usize = 1024 * 1024 * 1024;

/// controls when declared file contents can enter the process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileContentsMode {
    #[default]
    Supplied,
    /// pauses missing-content opens; the host must refill the same immutable snapshots.
    /// the budget counts resident contents, not guest pages or peak host memory.
    /// budgets above one gib are rejected before initial contents are copied.
    OnDemand { cache_bytes: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileContentsRequest<'a> {
    /// a monotonically increasing token scoped to this process instance.
    pub id: u64,
    /// a catalog path, never authority to access an arbitrary host path.
    pub path: &'a [u8],
    pub size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupplyFileContentsError {
    Exited,
    NoPendingRequest,
    StaleRequest,
    SizeMismatch,
    CapacityExceeded,
}

pub(super) struct Cache {
    mode: FileContentsMode,
    pending: Option<Pending>,
    next_id: u64,
}

#[derive(Clone, Copy)]
struct Pending {
    id: u64,
    index: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            mode: FileContentsMode::Supplied,
            pending: None,
            next_id: 1,
        }
    }
}

impl Cache {
    pub(super) fn on_demand(&self) -> bool {
        matches!(self.mode, FileContentsMode::OnDemand { .. })
    }

    pub(super) fn limit(&self) -> usize {
        match self.mode {
            FileContentsMode::Supplied => MAX_BYTES,
            FileContentsMode::OnDemand { cache_bytes } => cache_bytes,
        }
    }
}

impl Directory {
    pub(in super::super) fn configure_file_contents(
        &mut self,
        mode: FileContentsMode,
    ) -> Result<(), LoadError> {
        if matches!(mode, FileContentsMode::OnDemand { cache_bytes } if cache_bytes > MAX_BYTES) {
            return Err(LoadError::InvalidProcessParameters);
        }
        self.content_cache.mode = mode;
        Ok(())
    }

    pub(in super::super) fn ensure_contents(
        &mut self,
        index: usize,
    ) -> Result<bool, DispatchError> {
        if self.files[index].contents.is_some() {
            return Ok(true);
        }
        if !self.content_cache.on_demand() {
            return Err(DispatchError::Unsupported);
        }
        let size = self.files[index].size;
        if size > self.content_cache.limit() as u64
            || self.pinned_bytes() as u64 + size > self.content_cache.limit() as u64
        {
            return Ok(false);
        }
        if let Some(pending) = self.content_cache.pending {
            return Err(if pending.index == index {
                DispatchError::FileContentsRequired
            } else {
                DispatchError::Unsupported
            });
        }
        let next_id = self
            .content_cache
            .next_id
            .checked_add(1)
            .ok_or(DispatchError::Unsupported)?;
        self.content_cache.pending = Some(Pending {
            id: self.content_cache.next_id,
            index,
        });
        self.content_cache.next_id = next_id;
        Err(DispatchError::FileContentsRequired)
    }

    fn pinned_bytes(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.readers != 0)
            .filter_map(|file| file.contents.as_ref())
            .map(|bytes| bytes.len())
            .sum()
    }

    fn supply_contents(
        &mut self,
        id: u64,
        bytes: Box<[u8]>,
    ) -> Result<(), SupplyFileContentsError> {
        let pending = self
            .content_cache
            .pending
            .ok_or(SupplyFileContentsError::NoPendingRequest)?;
        if id != pending.id {
            return Err(SupplyFileContentsError::StaleRequest);
        }
        if bytes.len() as u64 != self.files[pending.index].size {
            return Err(SupplyFileContentsError::SizeMismatch);
        }
        let limit = self.content_cache.limit();
        if bytes.len() > limit || self.pinned_bytes() > limit - bytes.len() {
            return Err(SupplyFileContentsError::CapacityExceeded);
        }
        let mut resident: usize = self
            .files
            .iter()
            .filter_map(|file| file.contents.as_ref())
            .map(|bytes| bytes.len())
            .sum();
        for file in &mut self.files {
            if resident <= limit - bytes.len() {
                break;
            }
            if file.readers == 0
                && let Some(evicted) = file.contents.take()
            {
                resident -= evicted.len();
            }
        }
        self.files[pending.index].contents = Some(bytes);
        self.content_cache.pending = None;
        Ok(())
    }
}

impl Process32 {
    /// borrows the declared snapshot needed by the paused process, if any.
    #[must_use]
    pub fn pending_file_contents(&self) -> Option<FileContentsRequest<'_>> {
        let pending = self.current_directory.content_cache.pending?;
        let file = &self.current_directory.files[pending.index];
        Some(FileContentsRequest {
            id: pending.id,
            path: &file.path,
            size: file.size,
        })
    }

    /// moves an immutable snapshot into the cache; unpinned snapshots may be evicted.
    /// the host must refill the same file bytes after eviction. rejected bytes are dropped.
    ///
    /// # errors
    /// rejects an exited process, missing/stale request, wrong length or exhausted capacity.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn supply_file_contents(
        &mut self,
        id: u64,
        bytes: Box<[u8]>,
    ) -> Result<(), SupplyFileContentsError> {
        if self.exit_code.is_some() {
            return Err(SupplyFileContentsError::Exited);
        }
        self.current_directory.supply_contents(id, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{FileContents, FileMetadata};

    fn directory() -> Directory {
        let files = [b"C:\\a", b"C:\\b", b"C:\\c"].map(|path| FileMetadata { path, size: 4 });
        let mut directory = Directory::new(b"C:\\", &[], &files).unwrap();
        directory
            .configure_file_contents(FileContentsMode::OnDemand { cache_bytes: 8 })
            .unwrap();
        directory
            .attach_contents(&[
                FileContents {
                    path: b"C:\\a",
                    bytes: b"aaaa",
                },
                FileContents {
                    path: b"C:\\b",
                    bytes: b"bbbb",
                },
            ])
            .unwrap();
        directory
    }

    fn pointers(directory: &Directory) -> Vec<Option<*const u8>> {
        directory
            .files
            .iter()
            .map(|file| file.contents.as_ref().map(|bytes| bytes.as_ptr()))
            .collect()
    }

    #[test]
    fn rejected_supply_keeps_cache_identity_and_success_moves_the_box_in_catalog_order() {
        let mut directory = directory();
        assert!(matches!(
            directory.ensure_contents(2),
            Err(DispatchError::FileContentsRequired)
        ));
        let before = pointers(&directory);
        for (id, bytes, error) in [
            (2, b"cccc".as_slice(), SupplyFileContentsError::StaleRequest),
            (1, b"ccc".as_slice(), SupplyFileContentsError::SizeMismatch),
        ] {
            assert_eq!(directory.supply_contents(id, bytes.into()), Err(error));
            assert_eq!(pointers(&directory), before);
            assert_eq!(directory.content_cache.pending.unwrap().id, 1);
        }
        directory.files[0].readers = 1;
        directory.files[1].readers = 1;
        assert_eq!(
            directory.supply_contents(1, Box::from(*b"cccc")),
            Err(SupplyFileContentsError::CapacityExceeded)
        );
        assert_eq!(pointers(&directory), before);
        directory.files[0].readers = 0;
        directory.files[1].readers = 0;
        let supplied: Box<[u8]> = Box::from(*b"cccc");
        let identity = supplied.as_ptr();
        directory.supply_contents(1, supplied).unwrap();
        assert_eq!(pointers(&directory), [None, before[1], Some(identity)]);
        assert!(directory.content_cache.pending.is_none());
        assert!(matches!(
            directory.ensure_contents(0),
            Err(DispatchError::FileContentsRequired)
        ));
        assert_eq!(directory.content_cache.pending.unwrap().id, 2);
    }

    #[test]
    fn pinned_capacity_and_token_exhaustion_do_not_issue_a_request() {
        let mut directory = directory();
        let before = pointers(&directory);
        directory.files[0].readers = 2;
        directory.files[1].readers = 1;
        assert!(matches!(directory.ensure_contents(2), Ok(false)));
        assert!(directory.content_cache.pending.is_none());
        assert_eq!(directory.content_cache.next_id, 1);
        directory.files[0].readers = 0;
        directory.files[1].readers = 0;
        directory.content_cache.next_id = u64::MAX;
        assert!(matches!(
            directory.ensure_contents(2),
            Err(DispatchError::Unsupported)
        ));
        assert!(directory.content_cache.pending.is_none());
        assert_eq!(directory.content_cache.next_id, u64::MAX);
        assert_eq!(pointers(&directory), before);
    }
}
