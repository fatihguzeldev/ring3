use super::{FileCallOutcome32, context::Find32, handles::metadata, memory};
use crate::{
    files::{ATTR_DIRECTORY, DirectoryEntry32, FileError32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{FileRuntime32, handles::KernelFileObject32},
    windows::files::FileApi32,
};

fn encode(
    entry: &DirectoryEntry32,
    wide: bool,
    cp: crate::windows::text::CodePage32,
) -> Result<Vec<u8>, FileError32> {
    let size = if wide { 592 } else { 320 };
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| FileError32::Allocation)?;
    bytes.resize(size, 0);
    bytes[..36].copy_from_slice(&metadata::attributes(entry.info));
    let name = memory::encoded(&entry.name, wide, true, cp)?;
    if name.len() > 260 * if wide { 2 } else { 1 } {
        return Err(FileError32::InvalidName);
    }
    bytes[44..44 + name.len()].copy_from_slice(&name);
    Ok(bytes)
}
fn case_sensitive_match(pattern: &[u16], name: &[u16]) -> bool {
    let pattern = if pattern == [42, 46, 42] {
        &[42][..]
    } else {
        pattern
    };
    let (mut p, mut n, mut star, mut after) = (0, 0, None, 0);
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == 63 || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == 42 {
            star = Some(p);
            p += 1;
            after = n;
        } else if let Some(at) = star {
            after += 1;
            n = after;
            p = at + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == 42 {
        p += 1;
    }
    p == pattern.len()
}

impl FileRuntime32 {
    pub(super) fn search_file(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        use FileApi32::*;
        if api == FindClose {
            let KernelFileObject32::Find(id) = self.handles.object(args[0])? else {
                return Err(FileError32::InvalidHandle);
            };
            let index = self
                .ordinary
                .finds
                .iter()
                .position(|find| find.id == id)
                .ok_or(FileError32::InvalidHandle)?;
            self.handles.remove(args[0])?;
            self.ordinary.finds.remove(index);
            return Ok(FileCallOutcome32::success(1));
        }
        let wide = api.name().ends_with('W');
        let size = if wide { 592 } else { 320 };
        if matches!(api, FindNextFileA | FindNextFileW) {
            let KernelFileObject32::Find(id) = self.handles.object(args[0])? else {
                return Err(FileError32::InvalidHandle);
            };
            let index = self
                .ordinary
                .finds
                .iter()
                .position(|find| find.id == id)
                .ok_or(FileError32::InvalidHandle)?;
            let find = &self.ordinary.finds[index];
            let entry = find
                .entries
                .get(find.cursor)
                .ok_or(FileError32::NoMoreFiles)?;
            let bytes = encode(entry, wide, self.ordinary.ansi)?;
            memory::output(memory, args[1], size)?;
            memory
                .write(GuestAddress(args[1]), &bytes)
                .map_err(|_| FileError32::NoAccess)?;
            self.ordinary.finds[index].cursor += 1;
            return Ok(FileCallOutcome32::success(1));
        }
        let extended = matches!(api, FindFirstFileExA | FindFirstFileExW);
        if extended && (args[1] > 1 || args[3] > 1 || args[4] != 0 || args[5] & !7 != 0) {
            return Err(FileError32::InvalidParameter);
        }
        let output = if extended { args[2] } else { args[1] };
        memory::output(memory, output, size)?;
        if self.ordinary.finds.len() == self.ordinary.find_limit {
            return Err(FileError32::Capacity);
        }
        let id = self
            .ordinary
            .sequence
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let handle = self.handles.prepare(0, false)?;
        let path = memory::path(memory, args[0], wide, self.ordinary.ansi)?;
        let mut entries = self.vfs.find(&path)?;
        if extended && args[3] == 1 {
            entries.retain(|entry| entry.info.attributes & ATTR_DIRECTORY != 0);
        }
        if extended && args[5] & 1 != 0 {
            let pattern = path
                .rsplit(|&unit| unit == 92 || unit == 47)
                .next()
                .unwrap_or(&path);
            entries.retain(|entry| case_sensitive_match(pattern, &entry.name));
        }
        let first = entries.first().ok_or(FileError32::NotFound)?;
        let bytes = encode(first, wide, self.ordinary.ansi)?;
        memory
            .write(GuestAddress(output), &bytes)
            .map_err(|_| FileError32::NoAccess)?;
        self.ordinary.finds.push(Find32 {
            id,
            entries,
            cursor: 1,
        });
        self.ordinary.sequence = id;
        let value = self.handles.commit(handle, KernelFileObject32::Find(id));
        Ok(FileCallOutcome32::success(value))
    }
}
