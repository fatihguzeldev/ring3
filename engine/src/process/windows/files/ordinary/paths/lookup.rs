use super::{FileCallOutcome32, memory};
use crate::{
    files::{ATTR_DIRECTORY, FileError32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn search_path(
        &self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let wide = api == FileApi32::SearchPathW;
        let mut name = memory::path(memory, args[1], wide)?;
        if args[2] != 0 {
            let extension = memory::path(memory, args[2], wide)?;
            if !extension.is_empty() && extension[0] != 46 {
                return Err(FileError32::InvalidParameter);
            }
            let start = name
                .iter()
                .rposition(|unit| matches!(*unit, 47 | 92))
                .map_or(0, |index| index + 1);
            if !name[start..].contains(&46) {
                name.try_reserve(extension.len())
                    .map_err(|_| FileError32::Allocation)?;
                name.extend(extension);
            }
        }
        let directories = if name.iter().any(|unit| matches!(*unit, 47 | 58 | 92)) {
            vec![Vec::new()]
        } else if args[0] != 0 {
            memory::path(memory, args[0], wide)?
                .split(|unit| *unit == 59)
                .map(<[u16]>::to_vec)
                .collect()
        } else {
            let environment = &self.ordinary.environment;
            let mut paths = Vec::new();
            paths
                .try_reserve_exact(environment.search_path.len() + 4)
                .map_err(|_| FileError32::Allocation)?;
            paths.extend([
                environment.application_directory.clone(),
                self.vfs.current_directory().to_vec(),
                environment.system_directory.clone(),
                environment.windows_directory.clone(),
            ]);
            paths.extend(environment.search_path.iter().cloned());
            paths
        };
        let mut found = None;
        for mut directory in directories {
            if !directory.is_empty() && directory.last() != Some(&92) {
                directory.push(92);
            }
            directory.extend_from_slice(&name);
            match self.vfs.path_info(&directory) {
                Ok(info) if info.attributes & ATTR_DIRECTORY == 0 => {
                    found = Some(self.vfs.file_path(info.id)?);
                    break;
                }
                Ok(_) | Err(FileError32::NotFound | FileError32::PathNotFound) => (),
                Err(error) => return Err(error),
            }
        }
        let path = found.ok_or(FileError32::NotFound)?;
        let bytes = memory::encoded(&path, wide, true)?;
        let length = bytes.len() / if wide { 2 } else { 1 };
        if (args[3] as usize) < length {
            return Ok(FileCallOutcome32::success(length as u32));
        }
        memory::output(memory, args[4], bytes.len())?;
        let index = path
            .iter()
            .rposition(|unit| *unit == 92)
            .map_or(0, |index| index + 1);
        let part = args[4]
            .checked_add(memory::encoded(&path[..index], wide, false)?.len() as u32)
            .ok_or(FileError32::NoAccess)?
            .to_le_bytes();
        if args[5] != 0 {
            memory::output(memory, args[5], 4)?;
        }
        let spans = [
            (GuestAddress(args[4]), &bytes[..]),
            (
                GuestAddress(args[5]),
                if args[5] != 0 { &part[..] } else { &[] },
            ),
        ];
        memory
            .write_spans(&spans)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(FileCallOutcome32::success(length as u32 - 1))
    }
}
