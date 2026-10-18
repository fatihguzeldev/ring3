use super::{ArtifactError, CompiledRegion, RegistryError, ResidentRegistry, UnitId};
use crate::{
    cpu::x86::decode::{CodeDecodeKey, CodeFetchContext},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE},
};

#[derive(Debug, Default)]
pub(super) struct EntryIndex {
    // logical entry pages select candidates; code and decode guards remain authoritative.
    pages: Vec<EntryPage>,
}

#[derive(Debug)]
struct EntryPage {
    page: u32,
    units: Vec<UnitId>,
}

pub(super) struct PreparedEntryPages {
    pages: Vec<u32>,
    additions: Vec<EntryPage>,
}

impl EntryIndex {
    pub(super) fn prepare(
        &mut self,
        region: &CompiledRegion,
    ) -> Result<PreparedEntryPages, RegistryError> {
        let mut pages = Vec::new();
        for address in region.executable_addresses() {
            pages
                .try_reserve(1)
                .map_err(|_| RegistryError::Allocation)?;
            pages.push(address.0 / PAGE_SIZE);
        }
        pages.sort_unstable();
        pages.dedup();
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
        for &page in &pages {
            match self.find(page) {
                Ok(index) => self.pages[index]
                    .units
                    .try_reserve(1)
                    .map_err(|_| RegistryError::Allocation)?,
                Err(_) => {
                    let mut units = Vec::new();
                    units
                        .try_reserve_exact(1)
                        .map_err(|_| RegistryError::Allocation)?;
                    additions.push(EntryPage { page, units });
                }
            }
        }
        // only capacities changed; dropping this preparation cannot publish a candidate.
        Ok(PreparedEntryPages { pages, additions })
    }

    pub(super) fn publish(&mut self, id: UnitId, prepared: PreparedEntryPages) -> Vec<u32> {
        for page in prepared.additions {
            let index = self
                .find(page.page)
                .expect_err("prepared entry page is new");
            self.pages.insert(index, page);
        }
        for &page in &prepared.pages {
            let index = self.find(page).expect("all entry pages were reserved");
            let units = &mut self.pages[index].units;
            debug_assert!(units.last().is_none_or(|last| last.get() < id.get()));
            units.push(id);
        }
        prepared.pages
    }

    pub(super) fn remove(&mut self, id: UnitId, pages: &[u32]) {
        for &page in pages {
            let index = self.find(page).expect("retained unit has an entry page");
            let units = &mut self.pages[index].units;
            let unit = units
                .binary_search_by_key(&id.get(), |unit| unit.get())
                .expect("retained entry page has its unit");
            units.remove(unit);
            if units.is_empty() {
                self.pages.remove(index);
            }
        }
    }

    pub(super) fn candidates(&self, pc: u32) -> &[UnitId] {
        self.find(pc / PAGE_SIZE)
            .map(|index| self.pages[index].units.as_slice())
            .unwrap_or(&[])
    }

    fn find(&self, page: u32) -> Result<usize, usize> {
        self.pages.binary_search_by_key(&page, |entry| entry.page)
    }
}

impl ResidentRegistry {
    pub fn lookup(&self, memory: &AddressSpace, pc: GuestAddress) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        let mut stale = false;
        for &id in self.entry_index.candidates(pc.0) {
            let unit = &self.entries[self.unit_index(id.get())?];
            if unit.region.matches_decode_key(CodeDecodeKey::FLAT)
                && unit.region.contains_executable(pc.0)
            {
                match unit.region.wasm_bytes(memory) {
                    Ok(_) => return Ok(unit.id),
                    Err(ArtifactError::CodeInvalidated) => stale = true,
                }
            }
        }
        Err(if stale {
            RegistryError::CodeInvalidated
        } else {
            RegistryError::NotFound { pc }
        })
    }

    pub(crate) fn lookup_bound(
        &self,
        memory: &AddressSpace,
        pc: GuestAddress,
        cs: CodeFetchContext,
    ) -> Result<UnitId, RegistryError> {
        self.check_memory(memory)?;
        let mut stale = false;
        let candidates = self.entry_index.candidates(pc.0);
        for material in [true, false] {
            for &id in candidates {
                let unit = &self.entries[self.unit_index(id.get())?];
                if !unit.region.matches_decode_key(cs.decode_key()) {
                    continue;
                }
                let matches = if material {
                    unit.region.contains_material(pc.0)
                } else {
                    unit.region.context_fault(pc.0).is_some()
                };
                if matches {
                    match unit.region.wasm_bytes(memory) {
                        Ok(_)
                            if material
                                || unit
                                    .region
                                    .context_fault(pc.0)
                                    .is_some_and(|fault| fault.remains_denied(memory, cs)) =>
                        {
                            return Ok(unit.id);
                        }
                        Ok(_) => {}
                        Err(ArtifactError::CodeInvalidated) => stale = true,
                    }
                }
            }
        }
        Err(if stale {
            RegistryError::CodeInvalidated
        } else {
            RegistryError::NotFound { pc }
        })
    }
}
