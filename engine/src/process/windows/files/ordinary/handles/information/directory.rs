use crate::{
    files::{ATTR_DIRECTORY, DirectoryEntry32, FileAccess32, FileError32, OpenFileId},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory},
    },
};

pub(in crate::process::windows::files::ordinary) struct DirectoryQuery32 {
    pub open: OpenFileId,
    entries: Vec<(DirectoryEntry32, u64, u64)>,
    cursor: usize,
}
fn record(
    entry: &DirectoryEntry32,
    changed: u64,
    allocation: u64,
    class: u32,
    index: u32,
) -> Result<Vec<u8>, FileError32> {
    let prefix = match class {
        10 | 11 => 104,
        14 | 15 => 68,
        19 | 20 => 88,
        _ => unreachable!(),
    };
    let length = prefix + entry.name.len() * 2;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| FileError32::Allocation)?;
    bytes.resize(length, 0);
    bytes[4..8].copy_from_slice(&index.to_le_bytes());
    for (index, time) in [
        entry.info.times.created,
        entry.info.times.accessed,
        entry.info.times.written,
        changed,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[8 + index * 8..16 + index * 8].copy_from_slice(&time.to_le_bytes());
    }
    bytes[40..48].copy_from_slice(&entry.info.size.to_le_bytes());
    bytes[48..56].copy_from_slice(&allocation.to_le_bytes());
    bytes[56..60].copy_from_slice(&entry.info.attributes.to_le_bytes());
    bytes[60..64].copy_from_slice(&((entry.name.len() * 2) as u32).to_le_bytes());
    if matches!(class, 10 | 11) {
        bytes[96..104].copy_from_slice(&entry.info.id.0.to_le_bytes());
    }
    if matches!(class, 19 | 20) {
        bytes[72..80].copy_from_slice(&entry.info.id.0.to_le_bytes());
    }
    for (index, unit) in entry.name.iter().enumerate() {
        bytes[prefix + index * 2..prefix + index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    Ok(bytes)
}
impl FileRuntime32 {
    pub(super) fn query_directory_information(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        if self.handles.access(args[0])? & FileAccess32::READ.0 == 0 {
            return Err(FileError32::AccessDenied);
        }
        let info = self.vfs.open_info(open)?;
        if info.attributes & ATTR_DIRECTORY == 0 {
            return Err(FileError32::NotDirectory);
        }
        let index = self
            .ordinary
            .directories
            .iter()
            .position(|query| query.open == open);
        let restart = matches!(args[1], 11 | 15 | 20) || index.is_none();
        let prepared = if restart {
            if index.is_none() && self.ordinary.directories.len() == self.ordinary.find_limit {
                return Err(FileError32::Capacity);
            }
            let mut path = self.vfs.file_path(info.id)?;
            if path.last() != Some(&92) {
                path.push(92);
            }
            path.push(42);
            let entries = self.vfs.find(&path)?;
            let mut snapshot = Vec::new();
            snapshot
                .try_reserve_exact(entries.len())
                .map_err(|_| FileError32::Allocation)?;
            for entry in entries {
                let changed = self.vfs.change_time(entry.info.id)?;
                let allocation = self.vfs.allocation_size(entry.info.id)?;
                snapshot.push((entry, changed, allocation));
            }
            Some(DirectoryQuery32 {
                open,
                entries: snapshot,
                cursor: 0,
            })
        } else {
            None
        };
        let query = prepared
            .as_ref()
            .unwrap_or_else(|| &self.ordinary.directories[index.unwrap()]);
        if query.cursor == query.entries.len() {
            return Err(FileError32::NoMoreFiles);
        }
        let mut bytes = Vec::new();
        let mut count = 0usize;
        let mut previous = None;
        for (entry, changed, allocation) in &query.entries[query.cursor..] {
            let next = record(
                entry,
                *changed,
                *allocation,
                args[1],
                (query.cursor + count) as u32,
            )?;
            let start = (bytes.len() + 7) & !7;
            if start + next.len() > args[3] as usize {
                break;
            }
            bytes
                .try_reserve(start + next.len() - bytes.len())
                .map_err(|_| FileError32::Allocation)?;
            bytes.resize(start, 0);
            if let Some(previous) = previous {
                let offset = (start - previous) as u32;
                bytes[previous..previous + 4].copy_from_slice(&offset.to_le_bytes());
            }
            bytes.extend_from_slice(&next);
            previous = Some(start);
            count += 1;
        }
        if count == 0 {
            return Ok(FileCallOutcome32::error(0, 234));
        }
        memory::output(memory, args[2], bytes.len())?;
        memory
            .write(GuestAddress(args[2]), &bytes)
            .map_err(|_| FileError32::NoAccess)?;
        if let Some(mut query) = prepared {
            query.cursor = count;
            if let Some(index) = index {
                self.ordinary.directories[index] = query;
            } else {
                self.ordinary.directories.push(query);
            }
        } else {
            self.ordinary.directories[index.unwrap()].cursor += count;
        }
        Ok(FileCallOutcome32::success(1))
    }
}
