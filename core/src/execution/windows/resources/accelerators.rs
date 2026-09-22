use std::collections::BTreeMap;

use super::{Access, DispatchError, GuestMemory, MemoryError, Resource, guest};

const MAX_TABLES: usize = 1024;
const MAX_ENTRIES: u32 = 4096;
const FIRST_HANDLE: u32 = 0x7700_0004;
const LAST_HANDLE: u32 = 0x77ff_fffc;

struct Table {
    resource: u32,
    entries: Vec<[u8; 6]>,
}

pub(super) struct Tables {
    loaded: BTreeMap<u32, Table>,
    next: u32,
}

impl Default for Tables {
    fn default() -> Self {
        Self {
            loaded: BTreeMap::new(),
            next: FIRST_HANDLE,
        }
    }
}

impl Tables {
    pub(super) fn load(
        &mut self,
        resource: &Resource,
        memory: &GuestMemory,
    ) -> Result<Option<u32>, DispatchError> {
        if let Some((&handle, _)) = self
            .loaded
            .iter()
            .find(|(_, table)| table.resource == resource.info)
        {
            return Ok(Some(handle));
        }
        if self.loaded.len() == MAX_TABLES || self.next > LAST_HANDLE {
            return Ok(None);
        }
        let entries = resource.accelerators(memory)?;
        let handle = self.next;
        self.loaded.insert(
            handle,
            Table {
                resource: resource.info,
                entries,
            },
        );
        self.next += 4;
        Ok(Some(handle))
    }

    pub(super) fn copy(
        &self,
        args: &[u32],
        memory: &mut GuestMemory,
    ) -> Result<u32, DispatchError> {
        let table = self
            .loaded
            .get(&args[0])
            .ok_or(DispatchError::Unsupported)?;
        if args[1] == 0 {
            return Ok(u32::try_from(table.entries.len()).expect("bounded entry count"));
        }
        if args[2].cast_signed() <= 0 {
            return Ok(0);
        }
        let count = usize::try_from(args[2])
            .expect("positive count fits usize")
            .min(table.entries.len());
        guest::check(memory, args[1], count * 6, Access::Write)?;
        for (index, entry) in table.entries[..count].iter().enumerate() {
            memory.write(u64::from(args[1]) + (index * 6) as u64, entry)?;
        }
        Ok(u32::try_from(count).expect("bounded entry count"))
    }
}

impl Resource {
    fn accelerators(&self, memory: &GuestMemory) -> Result<Vec<[u8; 6]>, DispatchError> {
        if self.size == 0 || self.size > MAX_ENTRIES * 8 || !self.size.is_multiple_of(8) {
            return Err(DispatchError::Unsupported);
        }
        let start = self
            .base
            .checked_add(self.data)
            .ok_or(MemoryError::AddressOverflow)?;
        guest::check(memory, start, self.size as usize, Access::Read)?;
        let mut entries = Vec::new();
        for offset in (0..self.size).step_by(8) {
            let flags = self.word(memory, offset)?;
            let key = self.word(memory, offset + 2)?;
            let command = self.word(memory, offset + 4)?;
            if flags & !0x9f != 0
                || (flags & 0x80 != 0) != (offset + 8 == self.size)
                || key > if flags & 1 == 0 { 0x7f } else { 0xff }
            {
                return Err(DispatchError::Unsupported);
            }
            let [key_low, key_high] = key.to_le_bytes();
            let [command_low, command_high] = command.to_le_bytes();
            entries.push([
                u8::try_from(flags & 0x1f).expect("masked flags fit byte"),
                0,
                key_low,
                key_high,
                command_low,
                command_high,
            ]);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_bounds_and_address_overflow_are_checked_before_entry_reads() {
        use crate::execution::Permissions;
        let mut memory = GuestMemory::new(8);
        memory
            .map_zeroed(0x1000, 32768, Permissions::READ_WRITE)
            .unwrap();
        memory
            .write(0x1000 + 32760, &0x80_u16.to_le_bytes())
            .unwrap();
        let mut resource = Resource {
            info: 1,
            base: 0x1000,
            data: 0,
            size: 32768,
        };
        let Ok(entries) = resource.accelerators(&memory) else {
            panic!("bounded table must load");
        };
        assert_eq!(entries.len(), 4096);
        assert!(entries.iter().all(|entry| *entry == [0; 6]));
        for size in [0, 7, 9, 32776, u32::MAX] {
            resource.size = size;
            assert!(matches!(
                resource.accelerators(&memory),
                Err(DispatchError::Unsupported)
            ));
        }
        resource.size = 8;
        resource.base = u32::MAX - 3;
        resource.data = 8;
        assert!(matches!(
            resource.accelerators(&memory),
            Err(DispatchError::Memory(MemoryError::AddressOverflow))
        ));
    }

    #[test]
    fn quota_and_identity_exhaustion_leave_tables_unchanged_but_allow_cache_hits() {
        let memory = GuestMemory::new(0);
        let resource = Resource {
            info: 1,
            base: 0,
            data: 0,
            size: 8,
        };
        for full in [false, true] {
            let mut tables = Tables::default();
            if full {
                for index in 0..MAX_TABLES {
                    tables.loaded.insert(
                        u32::try_from(index).unwrap(),
                        Table {
                            resource: 2,
                            entries: vec![[0; 6]],
                        },
                    );
                }
            } else {
                tables.next = LAST_HANDLE + 4;
            }
            let next = tables.next;
            let count = tables.loaded.len();
            assert!(matches!(tables.load(&resource, &memory), Ok(None)));
            assert_eq!(tables.next, next);
            assert_eq!(tables.loaded.len(), count);
            tables.loaded.insert(
                0,
                Table {
                    resource: 1,
                    entries: vec![[0; 6]],
                },
            );
            assert!(matches!(tables.load(&resource, &memory), Ok(Some(0))));
        }
    }
}
