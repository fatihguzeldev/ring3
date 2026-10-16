use super::{FileCallOutcome32, memory};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn query_path(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        let wide = api.name().ends_with('W');
        if matches!(
            api,
            GetFileAttributesA | GetFileAttributesW | GetFileAttributesExA | GetFileAttributesExW
        ) {
            let path = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
            let info = self.vfs.path_info(&path)?;
            if matches!(api, GetFileAttributesA | GetFileAttributesW) {
                return Ok(FileCallOutcome32::success(info.attributes));
            }
            if args[1] != 0 {
                return Err(FileError32::InvalidParameter);
            }
            let bytes = super::super::handles::metadata::attributes(info);
            memory::output(memory, args[2], bytes.len())?;
            memory
                .write(GuestAddress(args[2]), &bytes)
                .map_err(|_| FileError32::NoAccess)?;
            return Ok(FileCallOutcome32::success(1));
        }
        let mut directory_input = false;
        let (path, capacity, address, part) = match api {
            GetCurrentDirectoryA | GetCurrentDirectoryW => {
                (self.vfs.current_directory().to_vec(), args[0], args[1], 0)
            }
            GetFullPathNameA | GetFullPathNameW => {
                let input = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
                directory_input = input.last().is_some_and(|unit| matches!(*unit, 47 | 92));
                let mut path = self.vfs.full_path(&input)?;
                if directory_input && path.last() != Some(&92) {
                    path.push(92);
                }
                (path, args[1], args[2], args[3])
            }
            GetLongPathNameA | GetLongPathNameW | GetShortPathNameA | GetShortPathNameW => {
                let input = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
                let info = self.vfs.path_info(&input)?;
                (self.vfs.file_path(info.id)?, args[2], args[1], 0)
            }
            _ => return Err(FileError32::Unsupported),
        };
        let encoded = memory::encoded(&path, wide, true, self.ordinary.ansi)?;
        let units = (encoded.len() / if wide { 2 } else { 1 }) as u32;
        if capacity < units {
            return Ok(FileCallOutcome32::success(units));
        }
        memory::output(memory, address, encoded.len())?;
        let part_value = if part != 0 {
            memory::output(memory, part, 4)?;
            let index = path
                .iter()
                .rposition(|&unit| unit == 92)
                .map_or(0, |index| index + 1);
            if directory_input || index == path.len() {
                0
            } else {
                address
                    .checked_add(
                        memory::encoded(&path[..index], wide, false, self.ordinary.ansi)?.len()
                            as u32,
                    )
                    .ok_or(FileError32::NoAccess)?
            }
        } else {
            0
        };
        let part_bytes = part_value.to_le_bytes();
        let mut spans = [
            (GuestAddress(address), &encoded[..]),
            (GuestAddress(part), &part_bytes[..]),
        ];
        if part == 0 {
            spans[1] = (GuestAddress(0), &[]);
        }
        memory
            .write_spans(&spans)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(FileCallOutcome32::success(units - 1))
    }
}
