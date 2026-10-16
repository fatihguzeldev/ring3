use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory},
    },
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn final_file_path(
        &self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        if args[3] & !15 != 0 || !matches!(args[3] & 7, 0 | 1 | 2 | 4) {
            return Err(FileError32::InvalidParameter);
        }
        let open = self.handles.file(args[0])?;
        let path = if args[3] & 8 != 0 {
            self.vfs.opened_path(open)?
        } else {
            self.vfs.file_path(self.vfs.file_for_open(open)?)?
        };
        let mut result = match args[3] & 7 {
            0 => vec![92, 92, 63, 92],
            4 => Vec::new(),
            1 => return Err(FileError32::PathNotFound),
            2 => return Err(FileError32::Unsupported),
            _ => unreachable!(),
        };
        result
            .try_reserve(path.len())
            .map_err(|_| FileError32::Allocation)?;
        result.extend_from_slice(if args[3] & 7 == 4 { &path[2..] } else { &path });
        let bytes = memory::encoded(
            &result,
            api == FileApi32::GetFinalPathNameByHandleW,
            true,
            self.ordinary.ansi,
        )?;
        let units = bytes.len()
            / if api == FileApi32::GetFinalPathNameByHandleW {
                2
            } else {
                1
            };
        if (args[2] as usize) < units {
            return Ok(FileCallOutcome32::success(units as u32));
        }
        memory::output(memory, args[1], bytes.len())?;
        memory
            .write(GuestAddress(args[1]), &bytes)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(FileCallOutcome32::success(units as u32 - 1))
    }
}
