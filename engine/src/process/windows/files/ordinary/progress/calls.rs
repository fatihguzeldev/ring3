use super::{CopyStage32, PendingCopy32};
use crate::{
    files::FileError32,
    memory::AddressSpace,
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory},
    },
};

impl FileRuntime32 {
    pub(in crate::process::windows) fn complete_file_callback(
        &mut self,
        memory: &mut AddressSpace,
        continuation: u64,
        result: u32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let copy = *self
            .ordinary
            .copies
            .last()
            .filter(|copy| copy.continuation == continuation)
            .ok_or(FileError32::InvalidHandle)?;
        let cancelled = if copy.cancel != 0 {
            match memory::dword(memory, copy.cancel) {
                Ok(value) => value != 0,
                Err(error) => return self.abort_copy(copy, error),
            }
        } else {
            false
        };
        let action = if cancelled { 1 } else { result };
        if action > 3 {
            return self.abort_copy(copy, FileError32::InvalidParameter);
        }
        if action == 1 {
            self.check_copy_cleanup(copy, 1, true)?;
            let cancel = self
                .vfs
                .prepare_cancel_copy_destination(copy.destination_file)?;
            cancel.validate(&self.vfs)?;
            self.vfs.commit_namespace(cancel);
            self.finish_copy_cleanup(copy);
            return Ok(FileCallOutcome32::error(0, 1235));
        }
        if action == 2 {
            self.check_copy_cleanup(copy, 0, false)?;
            self.finish_copy_cleanup(copy);
            return Ok(FileCallOutcome32::error(0, 1235));
        }
        match copy.stage {
            CopyStage32::Start => self.continue_copy(copy, action == 3),
            CopyStage32::Finish => {
                self.check_copy_cleanup(copy, 0, false)?;
                self.finish_copy_cleanup(copy);
                Ok(FileCallOutcome32::success(1))
            }
        }
    }
    fn continue_copy(
        &mut self,
        mut copy: PendingCopy32,
        quiet: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        let prepared = (|| {
            let size = self.vfs.info(copy.source_file)?.size;
            self.check_file_lock(copy.source, 0, size, false)?;
            self.check_file_lock(copy.destination, 0, size, true)?;
            if self.mappings.minimum_file_size(copy.destination_file) > 0 {
                return Err(FileError32::UserMappedFile);
            }
            self.check_copy_cleanup(copy, 1, false)?;
            let prepared = self
                .vfs
                .prepare_copy_file(copy.source_file, copy.destination_file)?;
            prepared.validate(&self.vfs)?;
            Ok((prepared, size))
        })();
        let (prepared, size) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return self.abort_copy(copy, error),
        };
        self.vfs.commit_namespace(prepared);
        copy.size = size;
        if quiet {
            self.finish_copy_cleanup(copy);
            Ok(FileCallOutcome32::success(1))
        } else {
            copy.stage = CopyStage32::Finish;
            *self.ordinary.copies.last_mut().expect("pending copy") = copy;
            Ok(FileCallOutcome32::Callback(copy.callback()))
        }
    }
    fn abort_copy(
        &mut self,
        copy: PendingCopy32,
        error: FileError32,
    ) -> Result<FileCallOutcome32, FileError32> {
        self.check_copy_cleanup(copy, 0, false)?;
        self.finish_copy_cleanup(copy);
        Ok(FileCallOutcome32::error(0, error.win32()))
    }
}
