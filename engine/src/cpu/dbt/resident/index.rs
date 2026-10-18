use super::{CompiledRegion, RegistryError, UnitId};
use crate::memory::PAGE_SIZE;

#[derive(Debug, Default)]
pub(super) struct EntryIndex {
    pages: Vec<EntryPage>,
}

#[derive(Debug)]
struct EntryPage {
    page: u32,
    // equal offsets retain increasing unit ids; candidates stay contiguous without per-pc vectors.
    offsets: Vec<u16>,
    units: Vec<UnitId>,
}

pub(super) struct PreparedEntryPages {
    addresses: Vec<u32>,
    pages: Vec<u32>,
    additions: Vec<EntryPage>,
}

impl EntryIndex {
    pub(super) fn prepare(
        &mut self,
        region: &CompiledRegion,
    ) -> Result<PreparedEntryPages, RegistryError> {
        let mut addresses = Vec::new();
        for address in region.executable_addresses() {
            addresses
                .try_reserve(1)
                .map_err(|_| RegistryError::Allocation)?;
            addresses.push(address.0);
        }
        addresses.sort_unstable();
        addresses.dedup();
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(addresses.len())
            .map_err(|_| RegistryError::Allocation)?;
        for group in addresses.chunk_by(|a, b| a / PAGE_SIZE == b / PAGE_SIZE) {
            pages.push(group[0] / PAGE_SIZE);
        }
        let new_pages = pages
            .iter()
            .filter(|&&page| self.find(page).is_err())
            .count();
        self.pages
            .try_reserve(new_pages)
            .map_err(|_| RegistryError::Allocation)?;
        let mut additions = Vec::new();
        additions
            .try_reserve_exact(new_pages)
            .map_err(|_| RegistryError::Allocation)?;
        for group in addresses.chunk_by(|a, b| a / PAGE_SIZE == b / PAGE_SIZE) {
            let page = group[0] / PAGE_SIZE;
            let entries = group.len();
            match self.find(page) {
                Ok(index) => self.pages[index].reserve(entries)?,
                Err(_) => {
                    let mut entry = EntryPage {
                        page,
                        offsets: Vec::new(),
                        units: Vec::new(),
                    };
                    entry.reserve(entries)?;
                    additions.push(entry);
                }
            }
        }
        // only capacities changed; abandoned preparation cannot publish candidates.
        Ok(PreparedEntryPages {
            addresses,
            pages,
            additions,
        })
    }

    pub(super) fn publish(&mut self, id: UnitId, prepared: PreparedEntryPages) -> Vec<u32> {
        for page in prepared.additions {
            let index = self
                .find(page.page)
                .expect_err("prepared entry page is new");
            self.pages.insert(index, page);
        }
        for address in prepared.addresses {
            let index = self
                .find(address / PAGE_SIZE)
                .expect("entry page was reserved");
            let page = &mut self.pages[index];
            let offset = (address % PAGE_SIZE) as u16;
            let at = page.offsets.partition_point(|&entry| entry <= offset);
            debug_assert!(
                at == 0 || page.offsets[at - 1] != offset || page.units[at - 1].get() < id.get()
            );
            page.offsets.insert(at, offset);
            page.units.insert(at, id);
        }
        prepared.pages
    }

    pub(super) fn remove(&mut self, id: UnitId, pages: &[u32]) {
        for &page in pages {
            let index = self.find(page).expect("retained unit has an entry page");
            let page = &mut self.pages[index];
            let mut kept = 0;
            for read in 0..page.units.len() {
                if page.units[read] != id {
                    page.offsets[kept] = page.offsets[read];
                    page.units[kept] = page.units[read];
                    kept += 1;
                }
            }
            debug_assert!(kept < page.units.len());
            page.offsets.truncate(kept);
            page.units.truncate(kept);
            if kept == 0 {
                self.pages.remove(index);
            }
        }
    }

    pub(super) fn candidates(&self, pc: u32) -> &[UnitId] {
        let Ok(index) = self.find(pc / PAGE_SIZE) else {
            return &[];
        };
        let page = &self.pages[index];
        let offset = (pc % PAGE_SIZE) as u16;
        let start = page.offsets.partition_point(|&entry| entry < offset);
        let count = page.offsets[start..].partition_point(|&entry| entry == offset);
        &page.units[start..start + count]
    }

    fn find(&self, page: u32) -> Result<usize, usize> {
        self.pages.binary_search_by_key(&page, |entry| entry.page)
    }
}

impl EntryPage {
    fn reserve(&mut self, entries: usize) -> Result<(), RegistryError> {
        self.offsets
            .try_reserve(entries)
            .map_err(|_| RegistryError::Allocation)?;
        self.units
            .try_reserve(entries)
            .map_err(|_| RegistryError::Allocation)
    }
}
