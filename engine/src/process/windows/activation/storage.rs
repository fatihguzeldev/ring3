use crate::{
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, PageRange, Permissions},
    windows::activation::{ActivationError32 as E, ActivationSection32},
};

pub(super) struct ContextStorage32 {
    pub(super) handle: u32,
    pub(super) sections: Vec<StoredSection32>,
    pub(super) range: Option<PageRange>,
    identity: u64,
    mapping: u64,
    pub(super) cost: usize,
}
pub(super) struct StoredSection32 {
    pub(super) offset: u32,
    pub(super) section: ActivationSection32,
}
pub(super) struct PreparedContextStorage32 {
    storage: ContextStorage32,
    bytes: Vec<u8>,
}
impl PreparedContextStorage32 {
    pub(super) fn prepare(
        memory: &AddressSpace,
        bounds: PageRange,
        handle: u32,
        sections: Vec<ActivationSection32>,
        source_cost: usize,
        budget: usize,
    ) -> Result<Self, E> {
        let mut bytes = Vec::new();
        let mut stored = Vec::new();
        stored
            .try_reserve_exact(sections.len())
            .map_err(|_| E::NotEnoughMemory)?;
        for section in sections {
            let length = (bytes.len() + 3) & !3;
            let end = length
                .checked_add(section.bytes.len())
                .filter(|&end| end <= budget)
                .ok_or(E::NotEnoughMemory)?;
            bytes
                .try_reserve(end - bytes.len())
                .map_err(|_| E::NotEnoughMemory)?;
            bytes.resize(length, 0);
            stored.push(StoredSection32 {
                offset: length.try_into().map_err(|_| E::NotEnoughMemory)?,
                section,
            });
            bytes.extend_from_slice(&stored.last().unwrap().section.bytes);
        }
        let page_count = bytes.len().div_ceil(PAGE_SIZE as usize);
        let cost = page_count
            .checked_mul(PAGE_SIZE as usize)
            .and_then(|length| length.checked_add(source_cost))
            .filter(|&length| length <= budget)
            .ok_or(E::NotEnoughMemory)?;
        let range = if page_count == 0 {
            None
        } else {
            Some(
                memory
                    .find_free_range(
                        bounds,
                        page_count.try_into().map_err(|_| E::NotEnoughMemory)?,
                        PAGE_SIZE,
                    )
                    .map_err(|_| E::NotEnoughMemory)?,
            )
        };
        Ok(Self {
            storage: ContextStorage32 {
                handle,
                sections: stored,
                range,
                identity: memory.identity(),
                mapping: 0,
                cost,
            },
            bytes,
        })
    }
    pub(super) fn stage(
        mut self,
        memory: &AddressSpace,
        bounds: PageRange,
        plan: &mut crate::loader::LoadPlan32,
    ) -> Result<ContextStorage32, E> {
        if let Some(range) = self.storage.range {
            let end = bounds.first + bounds.count;
            let mut start = bounds.first;
            let selected = loop {
                let search = PageRange::new(
                    GuestAddress(
                        (start * PAGE_SIZE as usize)
                            .try_into()
                            .map_err(|_| E::NotEnoughMemory)?,
                    ),
                    (end - start).try_into().map_err(|_| E::NotEnoughMemory)?,
                )
                .map_err(|_| E::NotEnoughMemory)?;
                let candidate = memory
                    .find_free_range(
                        search,
                        range.count.try_into().map_err(|_| E::NotEnoughMemory)?,
                        PAGE_SIZE,
                    )
                    .map_err(|_| E::NotEnoughMemory)?;
                if !plan.intersects_staged_range(candidate) {
                    break candidate;
                }
                start = candidate.first + 1;
                if start >= end {
                    return Err(E::NotEnoughMemory);
                }
            };
            self.storage.range = Some(selected);
            for (index, bytes) in self.bytes.chunks(PAGE_SIZE as usize).enumerate() {
                plan.add_page(
                    GuestAddress((selected.first + index) as u32 * PAGE_SIZE),
                    Permissions::READ,
                    bytes,
                )
                .map_err(|_| E::NotEnoughMemory)?;
            }
        }
        Ok(self.storage)
    }
    pub(super) fn publish(mut self, memory: &mut AddressSpace) -> Result<ContextStorage32, E> {
        if memory.identity() != self.storage.identity {
            return Err(E::Stale);
        }
        if let Some(range) = self.storage.range {
            let mut pages = Vec::new();
            pages
                .try_reserve_exact(range.count)
                .map_err(|_| E::NotEnoughMemory)?;
            for (index, bytes) in self.bytes.chunks(PAGE_SIZE as usize).enumerate() {
                pages.push(PageInitialization {
                    address: GuestAddress((range.first + index) as u32 * PAGE_SIZE),
                    permissions: Permissions::READ,
                    bytes,
                });
            }
            memory
                .install_pages_with_word_writes(&pages, &[])
                .map_err(|_| E::NoAccess)?;
            self.storage.mapping = memory
                .mapping_version_at(GuestAddress(range.first as u32 * PAGE_SIZE))
                .expect("activation sections were installed");
        }
        Ok(self.storage)
    }
}
impl ContextStorage32 {
    pub(super) fn adopt_mapping(&mut self, memory: &AddressSpace) {
        assert_eq!(memory.identity(), self.identity);
        if let Some(range) = self.range {
            self.mapping = memory
                .mapping_version_at(GuestAddress(range.first as u32 * PAGE_SIZE))
                .expect("module activation sections installed in the same RAM transaction");
        }
        self.check(memory)
            .expect("module activation mapping publication is coherent");
    }
    pub(super) fn check(&self, memory: &AddressSpace) -> Result<(), E> {
        if memory.identity() != self.identity {
            return Err(E::Stale);
        }
        if let Some(range) = self.range {
            for page in range.indices() {
                let address = GuestAddress(page as u32 * PAGE_SIZE);
                if memory.mapping_version_at(address).map_err(|_| E::Stale)? != self.mapping
                    || memory.permissions_at(address).map_err(|_| E::Stale)? != Permissions::READ
                {
                    return Err(E::Stale);
                }
            }
        }
        Ok(())
    }
    pub(super) fn base(&self) -> u32 {
        self.range.map_or(0, |range| range.first as u32 * PAGE_SIZE)
    }
}
