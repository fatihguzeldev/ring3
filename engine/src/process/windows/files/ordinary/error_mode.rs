use super::{FileCallOutcome32, memory};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(super) fn file_error_mode(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        match api {
            GetErrorMode => Ok(FileCallOutcome32::success(self.ordinary.error_mode)),
            GetThreadErrorMode => Ok(FileCallOutcome32::success(self.ordinary.thread_error_mode)),
            SetErrorMode => {
                let old = self.ordinary.error_mode;
                self.ordinary.error_mode = (args[0] & 0x8007) | (old & 4);
                Ok(FileCallOutcome32::success(old))
            }
            SetThreadErrorMode => {
                if args[0] & !0x8003 != 0 {
                    return Err(FileError32::InvalidParameter);
                }
                if args[1] != 0 {
                    memory::output(memory, args[1], 4)?;
                    memory
                        .write(
                            GuestAddress(args[1]),
                            &self.ordinary.thread_error_mode.to_le_bytes(),
                        )
                        .map_err(|_| FileError32::NoAccess)?;
                }
                self.ordinary.thread_error_mode = args[0];
                Ok(FileCallOutcome32::success(1))
            }
            _ => Err(FileError32::InvalidParameter),
        }
    }
}
