use super::{PeResources32, ResourceError32 as E, ResourceIdentifier32};
use std::cmp::Ordering;
#[derive(Clone, Copy)]
pub(super) struct DirectoryTable32 {
    pub entries_rva: u32,
    pub named: u32,
    pub ids: u32,
}
#[derive(Clone, Copy)]
pub(super) struct DirectoryEntry32 {
    pub name: u32,
    pub target: u32,
}
impl PeResources32<'_> {
    pub(super) fn table(&mut self, offset: u32) -> Result<DirectoryTable32, E> {
        if !offset.is_multiple_of(4) {
            return Err(E::Malformed);
        }
        let rva = self.directory_span(offset, 16)?;
        let header = self.read::<16>(rva)?;
        let named = u32::from(u16::from_le_bytes(header[12..14].try_into().unwrap()));
        let ids = u32::from(u16::from_le_bytes(header[14..16].try_into().unwrap()));
        let count = named + ids;
        self.entries = self
            .entries
            .checked_sub(count as usize)
            .ok_or(E::Capacity)?;
        let entries_rva = self.directory_span(
            offset.checked_add(16).ok_or(E::Malformed)?,
            count.checked_mul(8).ok_or(E::Malformed)?,
        )?;
        Ok(DirectoryTable32 {
            entries_rva,
            named,
            ids,
        })
    }
    pub(super) fn entry(&self, table: DirectoryTable32, index: u32) -> Result<DirectoryEntry32, E> {
        if index >= table.named + table.ids {
            return Err(E::Malformed);
        }
        let bytes = self.read::<8>(table.entries_rva + index * 8)?;
        let name = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let target = u32::from_le_bytes(bytes[4..].try_into().unwrap());
        if (name & 0x80000000 != 0) != (index < table.named) {
            return Err(E::Malformed);
        }
        Ok(DirectoryEntry32 { name, target })
    }
    pub(super) fn entry_name(
        &mut self,
        entry: DirectoryEntry32,
    ) -> Result<ResourceIdentifier32, E> {
        if entry.name & 0x80000000 != 0 {
            Ok(ResourceIdentifier32::Name(
                self.name(entry.name & 0x7fffffff)?,
            ))
        } else {
            Ok(ResourceIdentifier32::Id(entry.name))
        }
    }
    pub(super) fn child(&self, entry: DirectoryEntry32) -> Result<u32, E> {
        if entry.target & 0x80000000 == 0 {
            return Err(E::Malformed);
        }
        let offset = entry.target & 0x7fffffff;
        self.directory_span(offset, 16)?;
        Ok(offset)
    }
    pub(super) fn find_entry(
        &mut self,
        table: DirectoryTable32,
        name: &ResourceIdentifier32,
    ) -> Result<Option<DirectoryEntry32>, E> {
        let (mut low, mut high) = match name {
            ResourceIdentifier32::Id(_) => (table.named, table.named + table.ids),
            ResourceIdentifier32::Name(_) => (0, table.named),
        };
        while low < high {
            let middle = low + (high - low) / 2;
            let entry = self.entry(table, middle)?;
            let order = match name {
                ResourceIdentifier32::Id(id) => entry.name.cmp(id),
                ResourceIdentifier32::Name(name) => {
                    let (rva, length) = self.string_length(entry.name & 0x7fffffff)?;
                    let mut order = Ordering::Equal;
                    for (index, &unit) in name.iter().take(length).enumerate() {
                        order = self.half(rva + index as u32 * 2)?.cmp(&unit);
                        if order != Ordering::Equal {
                            break;
                        }
                    }
                    if order == Ordering::Equal {
                        length.cmp(&name.len())
                    } else {
                        order
                    }
                }
            };
            match order {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Ok(Some(entry)),
            }
        }
        Ok(None)
    }
}
