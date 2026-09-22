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

#[cfg(test)]
#[path = "../../../../tests/support/icon_executable.rs"]
mod icon_executable;
#[cfg(test)]
#[path = "../../../../tests/support/imported_executable.rs"]
mod imported_executable;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{Permissions, Process32};

    #[test]
    fn selected_palette_and_both_masks_are_owned_after_guest_bytes_change() {
        let bytes = icon_executable::guest();
        let mut p = Process32::load(&bytes, 32).unwrap();
        assert!(matches!(
            p.resources
                .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
            Ok(FIRST_HANDLE)
        ));
        let selected = &bytes[0xab8..0x1360];
        assert_eq!(p.resources.icons.loaded[&FIRST_HANDLE].1, selected);
        p.memory
            .write(icon_executable::SECOND_IMAGE, &vec![0xff; selected.len()])
            .unwrap();
        assert!(matches!(
            p.resources
                .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
            Ok(FIRST_HANDLE)
        ));
        assert_eq!(p.resources.icons.loaded[&FIRST_HANDLE].1, selected);
        assert_eq!(p.resources.icons.loaded.len(), 1);
    }

    #[test]
    fn group_selection_keeps_first_tie_and_does_not_hide_unsupported_best_depth() {
        let mut bytes = icon_executable::guest()[0x1360..0x1382].to_vec();
        assert!(matches!(select(&bytes), Ok(Entry { bits: 8, id: 2, .. })));
        bytes[26..28].copy_from_slice(&16_u16.to_le_bytes());
        assert!(matches!(
            select(&bytes),
            Ok(Entry {
                bits: 16,
                id: 2,
                ..
            })
        ));
        bytes[26..28].copy_from_slice(&4_u16.to_le_bytes());
        assert!(matches!(select(&bytes), Ok(Entry { bits: 4, id: 1, .. })));
        bytes[6..8].copy_from_slice(&[16, 16]);
        assert!(matches!(select(&bytes), Ok(Entry { id: 2, .. })));
        bytes[20..22].copy_from_slice(&[48, 48]);
        assert!(matches!(select(&bytes), Err(DispatchError::Unsupported)));
        for size in [0, 5, 6, 19, 33] {
            assert!(matches!(
                select(&bytes[..size]),
                Err(DispatchError::Unsupported)
            ));
        }
    }

    fn dib(bits: u16, colors: u32) -> Vec<u8> {
        let mut bytes = vec![0; 40 + colors as usize * 4 + 128 * usize::from(bits) + 128];
        for (offset, value) in [(0, 40), (4, 32), (8, 64), (32, colors)] {
            icon_executable::put(&mut bytes, offset, value);
        }
        bytes[12..14].copy_from_slice(&1_u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&bits.to_le_bytes());
        bytes
    }

    #[test]
    fn dib_extent_and_palette_indices_are_validated_independently_of_declared_size() {
        for (bits, colors) in [(1, 2), (4, 16), (8, 256), (24, 0), (32, 0)] {
            let mut bytes = dib(bits, colors);
            for declared in [0, 128 * u32::from(bits), 128 * u32::from(bits) + 128] {
                icon_executable::put(&mut bytes, 20, declared);
                assert!(validate(&bytes, bits).is_ok());
            }
            bytes.pop();
            assert!(matches!(
                validate(&bytes, bits),
                Err(DispatchError::Unsupported)
            ));
        }
        for bits in [1, 4, 8] {
            let mut bytes = dib(bits, 1);
            assert!(validate(&bytes, bits).is_ok());
            bytes[44] = 1 << (8 - bits);
            assert!(matches!(
                validate(&bytes, bits),
                Err(DispatchError::Unsupported)
            ));
        }
        let good = dib(8, 256);
        for (offset, value) in [
            (0, 108),
            (4, 0xffff_ffff),
            (8, 32),
            (12, 0x80002),
            (16, 3),
            (20, 1),
            (32, 257),
        ] {
            let mut bytes = good.clone();
            icon_executable::put(&mut bytes, offset, value);
            assert!(matches!(
                validate(&bytes, 8),
                Err(DispatchError::Unsupported)
            ));
        }
    }

    #[test]
    fn quota_and_handle_exhaustion_do_not_mutate_state_and_allow_cached_loads() {
        for quota in [false, true] {
            let mut p = Process32::load(&icon_executable::guest(), 32).unwrap();
            assert!(matches!(
                p.resources
                    .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
                Ok(FIRST_HANDLE)
            ));
            let cached = p.resources.icons.loaded.remove(&FIRST_HANDLE).unwrap();
            if quota {
                for n in 0..MAX_ICONS {
                    p.resources
                        .icons
                        .loaded
                        .insert(u32::try_from(n).unwrap(), (0, vec![]));
                }
            } else {
                p.resources.icons.next = LAST_HANDLE + 4;
            }
            let next = p.resources.icons.next;
            let len = p.resources.icons.loaded.len();
            p.memory
                .protect(0x7ffd_e000, 4096, Permissions::NONE)
                .unwrap();
            assert!(matches!(
                p.resources
                    .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
                Err(DispatchError::Memory(_))
            ));
            assert_eq!(
                (p.resources.icons.next, p.resources.icons.loaded.len()),
                (next, len)
            );
            p.memory
                .protect(0x7ffd_e000, 4096, Permissions::READ_WRITE)
                .unwrap();
            assert!(matches!(
                p.resources
                    .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
                Ok(0)
            ));
            assert_eq!(p.last_error().unwrap(), 8);
            p.resources.icons.loaded.insert(FIRST_HANDLE, cached);
            assert!(matches!(
                p.resources
                    .load_icon(&[0x0040_0000, 7], &p.modules, &mut p.memory),
                Ok(FIRST_HANDLE)
            ));
        }
    }

    #[test]
    fn resource_copy_checks_size_address_and_full_span_before_allocating() {
        let mut memory = GuestMemory::new(1);
        memory.map_zeroed(0x1000, 4096, Permissions::READ).unwrap();
        let mut resource = Resource {
            info: 1,
            base: 0x1000,
            data: 4092,
            size: 8,
        };
        assert!(matches!(
            resource.bytes(&memory, MAX_IMAGE),
            Err(DispatchError::Memory(_))
        ));
        resource.base = u32::MAX;
        assert!(matches!(
            resource.bytes(&memory, MAX_IMAGE),
            Err(DispatchError::Memory(MemoryError::AddressOverflow))
        ));
        for size in [0, 4265, u32::MAX] {
            resource.size = size;
            assert!(matches!(
                resource.bytes(&memory, MAX_IMAGE),
                Err(DispatchError::Unsupported)
            ));
        }
    }
}
