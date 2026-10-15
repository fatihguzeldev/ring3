use super::{FileCallOutcome32, memory};
use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    process::windows::files::FileRuntime32,
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn query_volume(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        let wide = api.name().ends_with('W');
        if api == GetLogicalDrives {
            return Ok(FileCallOutcome32::success(4));
        }
        if matches!(api, GetLogicalDriveStringsA | GetLogicalDriveStringsW) {
            let bytes = memory::encoded(&[67, 58, 92, 0], wide, true)?;
            if args[0] < 5 {
                return Ok(FileCallOutcome32::success(5));
            }
            memory::output(memory, args[1], bytes.len())?;
            memory
                .write(GuestAddress(args[1]), &bytes)
                .map_err(|_| FileError32::NoAccess)?;
            return Ok(FileCallOutcome32::success(4));
        }
        let path = if args[0] == 0 {
            self.vfs.current_directory().to_vec()
        } else {
            memory::path(memory, args[0], wide)?
        };
        let absolute = self.vfs.full_path(&path)?;
        if !absolute.starts_with(&[67, 58, 92]) {
            return if matches!(api, GetDriveTypeA | GetDriveTypeW) {
                Ok(FileCallOutcome32::success(1))
            } else {
                Err(FileError32::PathNotFound)
            };
        }
        if matches!(api, GetDriveTypeA | GetDriveTypeW) {
            return Ok(FileCallOutcome32::success(3));
        }
        let volume = self.vfs.volume();
        if matches!(api, GetDiskFreeSpaceA | GetDiskFreeSpaceW) {
            let values = [
                128u32,
                512,
                (volume.free_bytes / 65536).min(u64::from(u32::MAX)) as u32,
                (volume.total_bytes / 65536).min(u64::from(u32::MAX)) as u32,
            ]
            .map(u32::to_le_bytes);
            let mut spans = [(GuestAddress(0), &[][..]); 4];
            for index in 0..4 {
                memory::output(memory, args[index + 1], 4)?;
                spans[index] = (GuestAddress(args[index + 1]), &values[index]);
            }
            memory
                .write_spans(&spans)
                .map_err(|_| FileError32::NoAccess)?;
        } else if matches!(api, GetDiskFreeSpaceExA | GetDiskFreeSpaceExW) {
            self.vfs.path_info(&absolute)?;
            let values = [
                volume.free_bytes.to_le_bytes(),
                volume.total_bytes.to_le_bytes(),
                volume.free_bytes.to_le_bytes(),
            ];
            let mut spans = [(GuestAddress(0), &[][..]); 3];
            let mut count = 0;
            for index in 0..3 {
                if args[index + 1] != 0 {
                    memory::output(memory, args[index + 1], 8)?;
                    spans[count] = (GuestAddress(args[index + 1]), &values[index]);
                    count += 1;
                }
            }
            memory
                .write_spans(&spans[..count])
                .map_err(|_| FileError32::NoAccess)?;
        } else if matches!(api, GetVolumeInformationA | GetVolumeInformationW) {
            let name = memory::encoded(&[82, 105, 110, 103, 51], wide, true)?;
            let filesystem = memory::encoded(&[82, 73, 78, 71, 51, 70, 83], wide, true)?;
            if args[1] != 0 && args[2] < 6 || args[6] != 0 && args[7] < 8 {
                return Ok(FileCallOutcome32::error(0, 234));
            }
            let values = [
                volume.serial,
                volume.max_component,
                6 | if volume.readonly { 0x80000 } else { 0 },
            ]
            .map(u32::to_le_bytes);
            let mut spans = [(GuestAddress(0), &[][..]); 5];
            let mut count = 0;
            for (address, bytes) in [
                (args[1], &name[..]),
                (args[3], &values[0][..]),
                (args[4], &values[1][..]),
                (args[5], &values[2][..]),
                (args[6], &filesystem[..]),
            ] {
                if address != 0 {
                    memory::output(memory, address, bytes.len())?;
                    spans[count] = (GuestAddress(address), bytes);
                    count += 1;
                }
            }
            memory
                .write_spans(&spans[..count])
                .map_err(|_| FileError32::NoAccess)?;
        } else {
            return Err(FileError32::Unsupported);
        }
        Ok(FileCallOutcome32::success(1))
    }
}
