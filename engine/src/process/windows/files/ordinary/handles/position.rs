use super::{FileCallOutcome32, memory};
use crate::{
    files::{FileAccess32, FileError32, FileSeek32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn position_file(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        match api {
            FileApi32::SetFilePointer | FileApi32::SetFilePointerEx => {
                let extended = api == FileApi32::SetFilePointerEx;
                let (distance, out, method) = if extended {
                    (
                        ((u64::from(args[2]) << 32) | u64::from(args[1])) as i64,
                        args[3],
                        args[4],
                    )
                } else {
                    let high = if args[2] == 0 {
                        if args[1] & 0x8000_0000 != 0 {
                            u32::MAX
                        } else {
                            0
                        }
                    } else {
                        memory::dword(memory, args[2])?
                    };
                    (
                        ((u64::from(high) << 32) | u64::from(args[1])) as i64,
                        args[2],
                        args[3],
                    )
                };
                let method = match method {
                    0 => FileSeek32::Start,
                    1 => FileSeek32::Current,
                    2 => FileSeek32::End,
                    _ => return Err(FileError32::InvalidParameter),
                };
                if out != 0 {
                    memory::output(memory, out, if extended { 8 } else { 4 })?;
                }
                let prepared = self.vfs.prepare_seek(open, distance, method)?;
                let position = prepared.position();
                if !extended && out == 0 && position > u64::from(u32::MAX) {
                    return Err(FileError32::InvalidParameter);
                }
                prepared.validate(&self.vfs)?;
                if out != 0 {
                    if extended {
                        memory.write(GuestAddress(out), &position.to_le_bytes())
                    } else {
                        memory.write(GuestAddress(out), &((position >> 32) as u32).to_le_bytes())
                    }
                    .map_err(|_| FileError32::NoAccess)?;
                }
                self.vfs.commit_seek(prepared);
                Ok(FileCallOutcome32::Return {
                    value: if extended { 1 } else { position as u32 },
                    last_error: (!extended && position as u32 == u32::MAX).then_some(0),
                })
            }
            FileApi32::GetFileSize | FileApi32::GetFileSizeEx => {
                let size = self.vfs.open_info(open)?.size;
                let extended = api == FileApi32::GetFileSizeEx;
                if extended || args[1] != 0 {
                    memory::output(memory, args[1], if extended { 8 } else { 4 })?;
                    if extended {
                        memory.write(GuestAddress(args[1]), &size.to_le_bytes())
                    } else {
                        memory.write(GuestAddress(args[1]), &((size >> 32) as u32).to_le_bytes())
                    }
                    .map_err(|_| FileError32::NoAccess)?;
                }
                Ok(FileCallOutcome32::Return {
                    value: if extended { 1 } else { size as u32 },
                    last_error: (!extended && size as u32 == u32::MAX).then_some(0),
                })
            }
            FileApi32::SetEndOfFile => {
                if self.handles.access(args[0])? & FileAccess32::WRITE.0 == 0 {
                    return Err(FileError32::AccessDenied);
                }
                let file = self.vfs.file_for_open(open)?;
                let size = self.vfs.position(open)?;
                if size < self.mappings.minimum_file_size(file) {
                    return Err(FileError32::UserMappedFile);
                }
                let prepared = self.vfs.prepare_resize(file, size)?;
                prepared.validate(&self.vfs)?;
                self.vfs.commit_resize(prepared);
                Ok(FileCallOutcome32::success(1))
            }
            _ => Err(FileError32::InvalidParameter),
        }
    }
}
