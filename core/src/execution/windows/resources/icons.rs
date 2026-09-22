use std::collections::BTreeMap;

use super::super::{gdi, system};
use super::{
    Access, DispatchError, GuestMemory, MemoryError, Modules, Resource, Resources, Selection,
    guest, thread,
};

const MAX_ICONS: usize = 1024;
const MAX_GROUP: usize = 6 + 256 * 14;
const MAX_IMAGE: usize = 4264;
const FIRST_HANDLE: u32 = 0x7800_0004;
const LAST_HANDLE: u32 = 0x78ff_fffc;

pub(super) struct Icons {
    // shared handles own the selected dib, including its palette and both masks.
    loaded: BTreeMap<u32, (u32, Vec<u8>)>,
    next: u32,
}

impl Default for Icons {
    fn default() -> Self {
        Self {
            loaded: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Resources {
    pub(super) fn load_icon(
        &mut self,
        args: &[u32],
        modules: &Modules,
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        if args[0] == 0 || args[1] > 0xffff {
            return Err(DispatchError::Unsupported);
        }
        let group = match self.find(args[0], 14, args[1], modules)? {
            Selection::Found(resource) => resource,
            Selection::Missing(error) => return failure(memory, error),
        };
        if let Some((&handle, _)) = self
            .icons
            .loaded
            .iter()
            .find(|(_, (info, _))| *info == group.info)
        {
            return Ok(handle);
        }
        if self.icons.loaded.len() == MAX_ICONS || self.icons.next > LAST_HANDLE {
            return failure(memory, 8);
        }
        let entry = select(&group.bytes(memory, MAX_GROUP)?)?;
        let image = match self.find(args[0], 3, u32::from(entry.id), modules)? {
            Selection::Found(resource) => resource,
            Selection::Missing(error) => return failure(memory, error),
        };
        if image.size != entry.size {
            return Err(DispatchError::Unsupported);
        }
        let bytes = image.bytes(memory, MAX_IMAGE)?;
        validate(&bytes, entry.bits)?;
        let handle = self.icons.next;
        self.icons.loaded.insert(handle, (group.info, bytes));
        self.icons.next += 4;
        Ok(handle)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Entry {
    bits: u16,
    size: u32,
    id: u16,
}

fn select(bytes: &[u8]) -> Result<Entry, DispatchError> {
    if bytes.len() < 6 || bytes.len() > MAX_GROUP || word(bytes, 0) != 0 || word(bytes, 2) != 1 {
        return Err(DispatchError::Unsupported);
    }
    let count = usize::from(word(bytes, 4));
    if count == 0 || bytes.len() != 6 + count * 14 {
        return Err(DispatchError::Unsupported);
    }
    let mut selected = None;
    let rank = |bits: u16| {
        (
            bits > gdi::COLOR_BITS,
            if bits <= gdi::COLOR_BITS {
                u16::MAX - bits
            } else {
                bits
            },
        )
    };
    for data in bytes[6..].chunks_exact(14) {
        if data[3] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if u32::from(data[0]) != system::LARGE_ICON || u32::from(data[1]) != system::LARGE_ICON {
            continue;
        }
        let entry = Entry {
            bits: word(data, 6),
            size: dword(data, 8),
            id: word(data, 12),
        };
        if word(data, 4) != 1 || entry.bits == 0 || entry.size == 0 || entry.id == 0 {
            return Err(DispatchError::Unsupported);
        }
        if selected.is_none_or(|prior: Entry| rank(entry.bits) < rank(prior.bits)) {
            selected = Some(entry);
        }
    }
    selected.ok_or(DispatchError::Unsupported)
}

fn validate(bytes: &[u8], bits: u16) -> Result<(), DispatchError> {
    if bytes.len() < 40
        || ![1, 4, 8, 24, 32].contains(&bits)
        || dword(bytes, 0) != 40
        || dword(bytes, 4) != system::LARGE_ICON
        || dword(bytes, 8) != system::LARGE_ICON * 2
        || word(bytes, 12) != 1
        || word(bytes, 14) != bits
        || dword(bytes, 16) != 0
    {
        return Err(DispatchError::Unsupported);
    }
    let used = dword(bytes, 32);
    let colors = if bits <= 8 {
        if used == 0 { 1_u32 << bits } else { used }
    } else {
        0
    };
    if (bits <= 8 && colors > 1_u32 << bits) || (bits > 8 && used != 0) {
        return Err(DispatchError::Unsupported);
    }
    let size = system::LARGE_ICON as usize;
    let stride = (size * usize::from(bits)).div_ceil(32) * 4;
    let xor = stride * size;
    let mask = size.div_ceil(32) * 4 * size;
    let start = 40 + colors as usize * 4;
    if bytes.len() != start + xor + mask {
        return Err(DispatchError::Unsupported);
    }
    let declared = dword(bytes, 20) as usize;
    if declared != 0 && declared != xor && declared != xor + mask {
        return Err(DispatchError::Unsupported);
    }
    if bits <= 8 && used != 0 {
        for row in bytes[start..start + xor].chunks_exact(stride) {
            for x in 0..size {
                let bit = x * usize::from(bits);
                let index = (u32::from(row[bit / 8]) >> (8 - usize::from(bits) - bit % 8))
                    & ((1 << bits) - 1);
                if index >= colors {
                    return Err(DispatchError::Unsupported);
                }
            }
        }
    }
    Ok(())
}

impl Resource {
    fn bytes(&self, memory: &GuestMemory, maximum: usize) -> Result<Vec<u8>, DispatchError> {
        let size = self.size as usize;
        if size == 0 || size > maximum {
            return Err(DispatchError::Unsupported);
        }
        let start = self
            .base
            .checked_add(self.data)
            .ok_or(MemoryError::AddressOverflow)?;
        guest::check(memory, start, size, Access::Read)?;
        let mut bytes = vec![0; size];
        memory.read(u64::from(start), &mut bytes)?;
        Ok(bytes)
    }
}

fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("validated header span"),
    )
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated header span"),
    )
}

fn failure(memory: &mut GuestMemory, error: u32) -> Result<u32, DispatchError> {
    thread::set_last_error(memory, error)?;
    Ok(0)
}
