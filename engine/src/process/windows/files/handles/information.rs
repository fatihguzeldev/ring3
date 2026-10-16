use super::*;
use crate::process::{EngineInstance, HostError};
impl KernelFileHandles32 {
    pub(super) fn flags(&self, handle: u32) -> Result<u32, FileError32> {
        let entry = self.entry(handle)?;
        if matches!(entry.object, KernelFileObject32::Find(_)) {
            return Err(FileError32::InvalidHandle);
        }
        Ok(u32::from(entry.inherit) | u32::from(entry.protected) << 1)
    }
    pub(super) fn set_flags(
        &mut self,
        handle: u32,
        mask: u32,
        flags: u32,
    ) -> Result<(), FileError32> {
        if mask & !3 != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let old = self.flags(handle)?;
        let index = self.index(handle)?;
        let flags = (old & !mask) | (flags & mask);
        self.entries[index].inherit = flags & 1 != 0;
        self.entries[index].protected = flags & 2 != 0;
        Ok(())
    }
}
impl EngineInstance {
    pub(in crate::process) fn file_handle_flags(
        &self,
        handle: u32,
    ) -> Option<Result<u32, HostError>> {
        self.files
            .as_ref()
            .filter(|files| files.handles.object(handle).is_ok())
            .map(|files| files.handles.flags(handle).map_err(HostError::File))
    }
    pub(in crate::process) fn set_file_handle_flags(
        &mut self,
        handle: u32,
        mask: u32,
        flags: u32,
    ) -> Option<Result<(), HostError>> {
        self.files
            .as_mut()
            .filter(|files| files.handles.object(handle).is_ok())
            .map(|files| {
                files
                    .handles
                    .set_flags(handle, mask, flags)
                    .map_err(HostError::File)
            })
    }
}
