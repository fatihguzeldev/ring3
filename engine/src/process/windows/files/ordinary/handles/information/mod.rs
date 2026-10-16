mod directory;
mod encoding;
pub(in crate::process::windows::files::ordinary) use directory::DirectoryQuery32;
mod names;
mod setting;

use super::super::{FileCallOutcome32, memory};
use crate::{
    files::{ATTR_DIRECTORY, FileError32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn file_information_ex(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if matches!(args[1], 10 | 11 | 14 | 15 | 19 | 20) {
            return self.query_directory_information(memory, args);
        }
        let open = self.handles.file(args[0])?;
        let info = self.vfs.open_info(open)?;
        let allocation = self.vfs.allocation_size(info.id)?;
        let bytes = match args[1] {
            0 => encoding::basic(info, self.vfs.change_time(info.id)?),
            1 => encoding::standard(info, allocation),
            2 => {
                let path = self
                    .vfs
                    .file_path(info.id)
                    .or_else(|_| self.vfs.opened_path(open))?;
                encoding::name(&path[2..])?
            }
            7 => {
                if info.attributes & ATTR_DIRECTORY != 0 {
                    return Ok(FileCallOutcome32::error(0, 38));
                }
                encoding::stream(info.size, allocation)?
            }
            8 => encoding::compression(info.size),
            9 => {
                let mut bytes = vec![0; 8];
                bytes[..4].copy_from_slice(&info.attributes.to_le_bytes());
                bytes
            }
            16 => encoding::storage(),
            17 => 511u32.to_le_bytes().to_vec(),
            18 => {
                let mut bytes = vec![0; 24];
                bytes[..8].copy_from_slice(&u64::from(self.vfs.volume().serial).to_le_bytes());
                bytes[8..16].copy_from_slice(&info.id.0.to_le_bytes());
                bytes
            }
            23 => 0u32.to_le_bytes().to_vec(),
            _ => return Err(FileError32::Unsupported),
        };
        if (args[3] as usize) < bytes.len() {
            return Ok(FileCallOutcome32::error(0, 24));
        }
        memory::output(memory, args[2], bytes.len())?;
        memory
            .write(GuestAddress(args[2]), &bytes)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(FileCallOutcome32::success(1))
    }
}
