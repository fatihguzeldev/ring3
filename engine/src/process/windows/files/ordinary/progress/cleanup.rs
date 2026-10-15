use super::PendingCopy32;
use crate::{files::FileError32, process::windows::files::FileRuntime32};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn pending_copy_closes(
        &self,
        continuation: u64,
    ) -> Result<[(crate::files::OpenFileId, u32); 2], FileError32> {
        let copy = *self
            .ordinary
            .copies
            .last()
            .filter(|copy| copy.continuation == continuation)
            .ok_or(FileError32::InvalidHandle)?;
        Ok(self.copy_closes(copy))
    }

    pub(super) fn copy_closes(&self, copy: PendingCopy32) -> [(crate::files::OpenFileId, u32); 2] {
        [
            (
                copy.source,
                1 + u32::from(self.handles.file(copy.source_handle) == Ok(copy.source)),
            ),
            (
                copy.destination,
                1 + u32::from(self.handles.file(copy.destination_handle) == Ok(copy.destination)),
            ),
        ]
    }
    pub(super) fn check_copy_cleanup(
        &self,
        copy: PendingCopy32,
        mutations: u64,
        cancel: bool,
    ) -> Result<(), FileError32> {
        self.vfs.check_close_open_group(
            &self.copy_closes(copy),
            mutations,
            if cancel {
                std::slice::from_ref(&copy.destination_file)
            } else {
                &[]
            },
        )
    }
    pub(super) fn finish_copy_cleanup(&mut self, copy: PendingCopy32) {
        for (handle, open) in [
            (copy.source_handle, copy.source),
            (copy.destination_handle, copy.destination),
        ] {
            if self.handles.file(handle) == Ok(open) {
                self.handles.remove(handle).expect("prepared copy handle");
                self.vfs
                    .close_open(open)
                    .expect("prepared copy visible reference");
            }
            self.vfs
                .close_open(open)
                .expect("prepared copy hidden reference");
            if self.vfs.file_for_open(open).is_err() {
                self.release_file_locks(open);
            }
        }
        let removed = self.ordinary.copies.pop().expect("pending copy");
        assert_eq!(removed.continuation, copy.continuation);
    }
}
