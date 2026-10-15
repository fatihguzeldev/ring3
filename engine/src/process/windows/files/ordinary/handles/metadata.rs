use super::{FileCallOutcome32, memory};
use crate::{
    files::{FileAccess32, FileError32, FileInfo32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

pub(in crate::process::windows::files::ordinary) fn attributes(info: FileInfo32) -> [u8; 36] {
    let mut bytes = [0; 36];
    bytes[..4].copy_from_slice(&info.attributes.to_le_bytes());
    for (at, time) in [
        (4, info.times.created),
        (12, info.times.accessed),
        (20, info.times.written),
    ] {
        bytes[at..at + 8].copy_from_slice(&time.to_le_bytes());
    }
    bytes[28..32].copy_from_slice(&((info.size >> 32) as u32).to_le_bytes());
    bytes[32..36].copy_from_slice(&(info.size as u32).to_le_bytes());
    bytes
}

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn file_metadata(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        let info = self.vfs.open_info(open)?;
        match api {
            FileApi32::GetFileType => Ok(FileCallOutcome32::success(1)),
            FileApi32::FlushFileBuffers => {
                if self.handles.access(args[0])? & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0)
                    == 0
                {
                    return Err(FileError32::AccessDenied);
                }
                self.vfs.flush(open)?;
                Ok(FileCallOutcome32::success(1))
            }
            FileApi32::GetFileInformationByHandle => {
                let mut bytes = [0; 52];
                bytes[..28].copy_from_slice(&attributes(info)[..28]);
                bytes[28..32].copy_from_slice(&self.vfs.volume().serial.to_le_bytes());
                bytes[32..36].copy_from_slice(&((info.size >> 32) as u32).to_le_bytes());
                bytes[36..40].copy_from_slice(&(info.size as u32).to_le_bytes());
                bytes[40..44].copy_from_slice(&info.links.to_le_bytes());
                bytes[44..48].copy_from_slice(&((info.id.0 >> 32) as u32).to_le_bytes());
                bytes[48..52].copy_from_slice(&(info.id.0 as u32).to_le_bytes());
                memory::output(memory, args[1], 52)?;
                memory
                    .write(GuestAddress(args[1]), &bytes)
                    .map_err(|_| FileError32::NoAccess)?;
                Ok(FileCallOutcome32::success(1))
            }
            FileApi32::GetFileTime => {
                if self.handles.access(args[0])? & FileAccess32::READ_ATTRIBUTES.0 == 0 {
                    return Err(FileError32::AccessDenied);
                }
                let times = [
                    info.times.created.to_le_bytes(),
                    info.times.accessed.to_le_bytes(),
                    info.times.written.to_le_bytes(),
                ];
                let mut spans = [(GuestAddress(0), &[][..]); 3];
                let mut count = 0;
                for index in 0..3 {
                    if args[index + 1] != 0 {
                        memory::output(memory, args[index + 1], 8)?;
                        spans[count] = (GuestAddress(args[index + 1]), &times[index]);
                        count += 1;
                    }
                }
                memory
                    .write_spans(&spans[..count])
                    .map_err(|_| FileError32::NoAccess)?;
                Ok(FileCallOutcome32::success(1))
            }
            FileApi32::SetFileTime => {
                if self.handles.access(args[0])? & FileAccess32::WRITE_ATTRIBUTES.0 == 0 {
                    return Err(FileError32::AccessDenied);
                }
                let mut times = [None; 3];
                for (index, &address) in args[1..4].iter().enumerate() {
                    if address != 0 {
                        let value = u64::from_le_bytes(memory::read(memory, address)?);
                        if value != 0 {
                            times[index] = Some(value);
                        }
                    }
                }
                let prepared = self
                    .vfs
                    .prepare_file_times(open, times[0], times[1], times[2])?;
                prepared.validate(&self.vfs)?;
                self.vfs.commit_namespace(prepared);
                Ok(FileCallOutcome32::success(1))
            }
            _ => Err(FileError32::Unsupported),
        }
    }
}
