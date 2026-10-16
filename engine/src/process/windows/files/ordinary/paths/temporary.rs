use super::{FileCallOutcome32, memory};
use crate::{
    files::{ATTR_DIRECTORY, FileAccess32, FileDisposition32, FileError32, FileOpenOptions32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn temporary_path(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let wide = api.name().ends_with('W');
        if matches!(api, FileApi32::GetTempPathA | FileApi32::GetTempPathW) {
            let mut path = self.ordinary.environment.temp_directory.clone();
            if path.last() != Some(&92) {
                path.push(92);
            }
            let bytes = memory::encoded(&path, wide, true, self.ordinary.ansi)?;
            let length = bytes.len() / if wide { 2 } else { 1 };
            if (args[0] as usize) < length {
                return Ok(FileCallOutcome32::success(length as u32));
            }
            memory::output(memory, args[1], bytes.len())?;
            memory
                .write(GuestAddress(args[1]), &bytes)
                .map_err(|_| FileError32::NoAccess)?;
            return Ok(FileCallOutcome32::success(length as u32 - 1));
        }
        let directory = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
        if directory.len() > 246 {
            return Ok(FileCallOutcome32::error(0, 111));
        }
        let prefix = memory::path(memory, args[1], wide, self.ordinary.ansi)?;
        if self.vfs.path_info(&directory)?.attributes & ATTR_DIRECTORY == 0 {
            return Err(FileError32::NotDirectory);
        }
        let mut base = directory;
        if !base.last().is_some_and(|unit| matches!(*unit, 47 | 92)) {
            base.push(92);
        }
        base.extend(prefix.into_iter().take(3));
        let mut unique = if args[2] != 0 {
            args[2] as u16
        } else {
            self.ordinary.temp_sequence.wrapping_add(1).max(1)
        };
        for _ in 0..65535 {
            let mut candidate = base.clone();
            for shift in [12, 8, 4, 0] {
                let digit = ((unique >> shift) & 15) as u8;
                candidate.push(u16::from(if digit < 10 {
                    b'0' + digit
                } else {
                    b'A' + digit - 10
                }));
            }
            candidate.extend([46, 84, 77, 80]);
            let bytes = memory::encoded(&candidate, wide, true, self.ordinary.ansi)?;
            if bytes.len() / if wide { 2 } else { 1 } > 260 {
                return Ok(FileCallOutcome32::error(0, 111));
            }
            memory::output(memory, args[3], bytes.len())?;
            if args[2] != 0 {
                memory
                    .write(GuestAddress(args[3]), &bytes)
                    .map_err(|_| FileError32::NoAccess)?;
                return Ok(FileCallOutcome32::success(args[2]));
            }
            match self.vfs.prepare_open(
                &candidate,
                FileOpenOptions32 {
                    access: FileAccess32::WRITE,
                    disposition: FileDisposition32::CreateNew,
                    ..FileOpenOptions32::default()
                },
            ) {
                Ok(prepared) => {
                    self.vfs
                        .revision()
                        .checked_add(2)
                        .ok_or(FileError32::Exhausted)?;
                    prepared.validate(&self.vfs)?;
                    memory
                        .write(GuestAddress(args[3]), &bytes)
                        .map_err(|_| FileError32::NoAccess)?;
                    let open = self.vfs.commit_open(prepared).open;
                    self.vfs.close_open(open).expect("prepared temporary close");
                    self.ordinary.temp_sequence = unique;
                    return Ok(FileCallOutcome32::success(u32::from(unique)));
                }
                Err(FileError32::FileExists) => unique = unique.wrapping_add(1).max(1),
                Err(error) => return Err(error),
            }
        }
        Err(FileError32::FileExists)
    }
}
