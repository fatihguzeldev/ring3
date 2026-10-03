use super::{LoadError, pe32::ImagePlan};
use crate::memory::MemoryError;

const MAX_DIRECTORY_BYTES: u32 = 16 * 1024;
const MAX_BLOCKS: usize = 1024;
const MAX_ENTRIES: usize = 4096;

#[derive(Clone, Copy)]
pub(super) struct RelocationDirectory {
    pub rva: u32,
    pub size: u32,
}

pub(super) struct Fixup {
    pub rva: u32,
    pub value: u32,
}

pub(super) fn prepare_fixups(
    bytes: &[u8],
    image: &ImagePlan,
    directory: Option<RelocationDirectory>,
    delta: u32,
) -> Result<Vec<Fixup>, LoadError> {
    let Some(directory) = directory else {
        return Ok(Vec::new());
    };
    if directory.size > MAX_DIRECTORY_BYTES {
        return Err(LoadError::Capacity);
    }
    if !directory.rva.is_multiple_of(4) {
        return Err(LoadError::Malformed);
    }
    let range = image.file_range(directory.rva, directory.size)?;
    let table = &bytes[range];
    let mut fixups = Vec::new();
    fixups
        .try_reserve_exact((table.len() / 2).min(MAX_ENTRIES))
        .map_err(|_| LoadError::Memory(MemoryError::Allocation))?;
    let mut position = 0;
    let mut blocks = 0;
    let mut entries = 0;
    while position < table.len() {
        if !position.is_multiple_of(4) || table.len() - position < 8 {
            return Err(LoadError::Malformed);
        }
        blocks += 1;
        if blocks > MAX_BLOCKS {
            return Err(LoadError::Capacity);
        }
        let page = word(table, position);
        let size = word(table, position + 4) as usize;
        if !page.is_multiple_of(4096)
            || page >= image.image_size()
            || size < 8
            || !size.is_multiple_of(2)
            || size > table.len() - position
        {
            return Err(LoadError::Malformed);
        }
        entries += (size - 8) / 2;
        if entries > MAX_ENTRIES {
            return Err(LoadError::Capacity);
        }
        for offset in (position + 8..position + size).step_by(2) {
            let record = u16::from_le_bytes([table[offset], table[offset + 1]]);
            match record >> 12 {
                0 => continue,
                3 => {}
                _ => return Err(LoadError::Unsupported),
            }
            let target = page
                .checked_add(u32::from(record & 0x0fff))
                .ok_or(LoadError::Malformed)?;
            let target_end = u64::from(target) + 4;
            let directory_end = u64::from(directory.rva) + u64::from(directory.size);
            if u64::from(target) < directory_end && u64::from(directory.rva) < target_end {
                return Err(LoadError::Malformed);
            }
            let range = image.file_range(target, 4)?;
            let value = word(bytes, range.start).wrapping_add(delta);
            fixups.push(Fixup { rva: target, value });
        }
        position += size;
    }
    fixups.sort_unstable_by_key(|fixup| fixup.rva);
    if fixups
        .windows(2)
        .any(|pair| u64::from(pair[0].rva) + 4 > u64::from(pair[1].rva))
    {
        return Err(LoadError::Malformed);
    }
    Ok(fixups)
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
