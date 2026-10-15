use super::{FileCallOutcome32, memory};
use crate::{
    files::{FileAccess32, FileError32, FileIo32},
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn transfer_file(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[3] == 0 {
            return Err(FileError32::InvalidParameter);
        }
        memory::output(memory, args[3], 4)?;
        let result = self.transfer_ready(memory, api, args);
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                memory
                    .write(GuestAddress(args[3]), &[0; 4])
                    .map_err(|_| FileError32::NoAccess)?;
                Ok(FileCallOutcome32::error(0, error.win32()))
            }
        }
    }

    fn transfer_ready(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[4] != 0 {
            return Err(FileError32::InvalidParameter);
        }
        let open = self.handles.file(args[0])?;
        let options = self.vfs.open_options(open)?;
        if options.overlapped {
            return Err(FileError32::InvalidParameter);
        }
        let read = api == FileApi32::ReadFile;
        let access = self.handles.access(args[0])?;
        if if read {
            access & FileAccess32::READ.0 == 0
        } else {
            access & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0) == 0
        } {
            return Err(FileError32::AccessDenied);
        }
        let length = args[2] as usize;
        let offset = if !read && access & FileAccess32::WRITE.0 == 0 {
            self.vfs.open_info(open)?.size
        } else {
            self.vfs.position(open)?
        };
        self.check_file_lock(open, offset, args[2] as u64, !read)?;
        if options.no_buffering
            && (!args[1].is_multiple_of(512)
                || !args[2].is_multiple_of(512)
                || !offset.is_multiple_of(512))
        {
            return Err(FileError32::InvalidParameter);
        }
        if read {
            memory::output(memory, args[1], length)?;
            let prepared = match self.vfs.prepare_read(open, length)? {
                FileIo32::Ready(prepared) => prepared,
                FileIo32::NeedBytes(request) => {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
            };
            prepared.validate(&self.vfs)?;
            let count = (prepared.bytes().len() as u32).to_le_bytes();
            memory
                .write_spans(&[
                    (GuestAddress(args[1]), prepared.bytes()),
                    (GuestAddress(args[3]), &count),
                ])
                .map_err(|_| FileError32::NoAccess)?;
            self.vfs.commit_read(prepared);
        } else {
            if length != 0 {
                if args[1] == 0 {
                    return Err(FileError32::NoAccess);
                }
                memory
                    .check_span(GuestAddress(args[1]), length, Access::Read)
                    .map_err(|_| FileError32::NoAccess)?;
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(length)
                .map_err(|_| FileError32::Allocation)?;
            bytes.resize(length, 0);
            if length != 0 {
                memory
                    .read(GuestAddress(args[1]), &mut bytes)
                    .map_err(|_| FileError32::NoAccess)?;
            }
            let io = if access & FileAccess32::WRITE.0 == 0 {
                self.vfs.prepare_write_positioned(open, offset, &bytes)?
            } else {
                self.vfs.prepare_write(open, &bytes)?
            };
            let prepared = match io {
                FileIo32::Ready(prepared) => prepared,
                FileIo32::NeedBytes(request) => {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
            };
            prepared.validate(&self.vfs)?;
            let count = (prepared.length() as u32).to_le_bytes();
            memory
                .write(GuestAddress(args[3]), &count)
                .map_err(|_| FileError32::NoAccess)?;
            self.vfs.commit_write(prepared);
        }
        Ok(FileCallOutcome32::success(1))
    }
}
