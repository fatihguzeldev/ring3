use super::*;
use crate::{files::FileAccess32, memory::Access, windows::security::Token32};

impl FileRuntime32 {
    pub(in crate::process::windows::files::ordinary) fn set_file_information(
        &mut self,
        space: &AddressSpace,
        args: &[u32],
        token: &Token32,
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        let required = match args[1] {
            0 => 40,
            3 => 12,
            4 => 1,
            5 | 6 => 8,
            _ => return Err(FileError32::InvalidParameter),
        };
        if args[3] < required {
            return Ok(FileCallOutcome32::error(0, 24));
        }
        let right = match args[1] {
            0 => FileAccess32::WRITE_ATTRIBUTES,
            3 | 4 => FileAccess32::DELETE,
            _ => FileAccess32::WRITE,
        };
        if self.handles.access(args[0])? & right.0 == 0 {
            return Err(FileError32::AccessDenied);
        }
        match args[1] {
            0 => {
                let bytes = memory::read::<40>(space, args[2])?;
                let times = core::array::from_fn(|index| {
                    u64::from_le_bytes(bytes[index * 8..index * 8 + 8].try_into().unwrap())
                });
                let attributes = u32::from_le_bytes(bytes[32..36].try_into().unwrap());
                let prepared = self
                    .vfs
                    .prepare_basic_information(open, times, attributes)?;
                prepared.validate(&self.vfs)?;
                self.vfs.commit_namespace(prepared);
            }
            3 => {
                let bytes = memory::read::<12>(space, args[2])?;
                let root = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
                if length == 0
                    || length & 1 != 0
                    || length > 65534
                    || length.checked_add(12).is_none_or(|length| length > args[3])
                {
                    return Err(FileError32::InvalidParameter);
                }
                let address = args[2].checked_add(12).ok_or(FileError32::NoAccess)?;
                let mut name = read_name(space, address, length as usize)?;
                if root != 0 {
                    if name.first() == Some(&92) || name.get(1) == Some(&58) {
                        return Err(FileError32::InvalidParameter);
                    }
                    let directory = self.handles.file(root)?;
                    let info = self.vfs.open_info(directory)?;
                    if info.attributes & ATTR_DIRECTORY == 0 {
                        return Err(FileError32::NotDirectory);
                    }
                    let mut path = self.vfs.file_path(info.id)?;
                    path.try_reserve(name.len() + 1)
                        .map_err(|_| FileError32::Allocation)?;
                    if path.last() != Some(&92) {
                        path.push(92);
                    }
                    path.append(&mut name);
                    name = path;
                }
                let source = self.vfs.open_info(open)?;
                self.vfs.check_parent_access(
                    &name,
                    if source.attributes & ATTR_DIRECTORY != 0 {
                        4
                    } else {
                        2
                    },
                    token,
                )?;
                if self
                    .vfs
                    .path_info(&name)
                    .is_ok_and(|target| target.id != source.id)
                {
                    self.vfs.check_delete_access(&name, token)?;
                }
                let prepared = self.vfs.prepare_handle_rename(open, &name, bytes[0] != 0)?;
                prepared.validate(&self.vfs)?;
                self.vfs.commit_namespace(prepared);
            }
            4 => {
                let delete = memory::read::<1>(space, args[2])?[0] != 0;
                let prepared = self.vfs.prepare_disposition(open, delete)?;
                prepared.validate(&self.vfs)?;
                self.vfs.commit_namespace(prepared);
            }
            5 | 6 => {
                let size = u64::from_le_bytes(memory::read::<8>(space, args[2])?);
                if size > i64::MAX as u64 {
                    return Err(FileError32::InvalidParameter);
                }
                let file = self.vfs.file_for_open(open)?;
                if size < self.mappings.minimum_file_size(file) {
                    return Err(FileError32::UserMappedFile);
                }
                if args[1] == 5 {
                    let prepared = self.vfs.prepare_allocation(open, size)?;
                    prepared.validate(&self.vfs)?;
                    self.vfs.commit_allocation(prepared);
                } else {
                    let prepared = self.vfs.prepare_resize_open(open, size)?;
                    prepared.validate(&self.vfs)?;
                    self.vfs.commit_resize(prepared);
                }
            }
            _ => unreachable!(),
        }
        Ok(FileCallOutcome32::success(1))
    }
}

fn read_name(space: &AddressSpace, address: u32, length: usize) -> Result<Vec<u16>, FileError32> {
    space
        .check_span(GuestAddress(address), length, Access::Read)
        .map_err(|_| FileError32::NoAccess)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| FileError32::Allocation)?;
    bytes.resize(length, 0);
    space
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| FileError32::NoAccess)?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(length / 2)
        .map_err(|_| FileError32::Allocation)?;
    for pair in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes(pair.try_into().unwrap());
        if unit == 0 {
            return Err(FileError32::InvalidName);
        }
        units.push(unit);
    }
    Ok(units)
}
