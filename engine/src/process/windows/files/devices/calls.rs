use super::super::{
    FileCallOutcome32, FileRuntime32,
    handles::KernelFileObject32,
    ordinary::{access, memory as guest},
};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    windows::files::{FileApi32 as Api, devices::DeviceName32},
};
impl FileRuntime32 {
    pub(in crate::process::windows::files) fn open_device(
        &mut self,
        memory: &AddressSpace,
        name: DeviceName32,
        args: &[u32],
        console_attached: bool,
    ) -> Result<FileCallOutcome32, FileError32> {
        if name != DeviceName32::Null {
            return Err(if console_attached {
                FileError32::Unsupported
            } else {
                FileError32::NotFound
            });
        }
        if !(1..=5).contains(&args[4]) || args[6] != 0 || args[5] & !(0xc0000000 | 0x80) != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let inherit = guest::inherit(memory, args[3])?;
        let rights = access::desired_handle_access(args[1])?;
        let prepared = self.null_devices.prepare_open(args[5] & 0x40000000 != 0)?;
        let handle = self.handles.prepare(rights, inherit)?;
        let id = self.null_devices.commit_open(prepared);
        let handle = self.handles.commit(handle, KernelFileObject32::Null(id));
        Ok(FileCallOutcome32::success(handle))
    }
    pub(in crate::process::windows::files) fn null_metadata(
        &self,
        memory: &mut AddressSpace,
        api: Api,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let KernelFileObject32::Null(id) = self.handles.object(args[0])? else {
            return Err(FileError32::InvalidHandle);
        };
        self.null_devices.overlapped(id)?;
        match api {
            Api::GetFileType => Ok(FileCallOutcome32::success(2)),
            Api::GetFileSize => {
                if args[1] != 0 {
                    guest::output(memory, args[1], 4)?;
                    memory
                        .write(GuestAddress(args[1]), &[0; 4])
                        .map_err(|_| FileError32::NoAccess)?;
                }
                Ok(FileCallOutcome32::success(0))
            }
            Api::GetFileSizeEx => {
                guest::output(memory, args[1], 8)?;
                memory
                    .write(GuestAddress(args[1]), &[0; 8])
                    .map_err(|_| FileError32::NoAccess)?;
                Ok(FileCallOutcome32::success(1))
            }
            Api::SetFilePointer
            | Api::SetFilePointerEx
            | Api::SetEndOfFile
            | Api::FlushFileBuffers => Err(FileError32::InvalidFunction),
            _ => Err(FileError32::Unsupported),
        }
    }
}
