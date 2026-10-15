use super::ModuleRuntimeConfig32;
use crate::{
    loader::{LoadPlan32, ModuleError32, ModuleId32},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions, WordWrite32},
};

const TLS_POOL_BYTES: u32 = 16 * 1024 * 1024;

pub(super) struct TlsAllocation {
    pub module: ModuleId32,
    pub slot: u32,
    pub range: PageRange,
}

pub(super) struct InitialThreadTls {
    pub config: ModuleRuntimeConfig32,
    pub allocations: Vec<TlsAllocation>,
    slots: u32,
    array_pages: u32,
}

impl InitialThreadTls {
    pub fn new(config: ModuleRuntimeConfig32, slots: u32) -> Result<Self, ModuleError32> {
        if slots == 0 {
            return Err(ModuleError32::Capacity);
        }
        let array_pages = (u64::from(slots) * 4).div_ceil(u64::from(PAGE_SIZE)) as u32;
        let stack_end =
            u64::from(config.stack_base) + u64::from(config.stack_pages) * u64::from(PAGE_SIZE);
        let runtime_end = u64::from(config.runtime_base) + u64::from(TLS_POOL_BYTES);
        if config.stack_base == 0
            || !config.stack_base.is_multiple_of(PAGE_SIZE)
            || !(1..=4096).contains(&config.stack_pages)
            || stack_end > 1 << 32
            || config.runtime_base == 0
            || !config.runtime_base.is_multiple_of(65536)
            || runtime_end > 1 << 32
            || (u64::from(config.stack_base) < runtime_end
                && u64::from(config.runtime_base) < stack_end)
        {
            return Err(ModuleError32::InvalidRequest);
        }
        Ok(Self {
            config,
            allocations: Vec::new(),
            slots,
            array_pages,
        })
    }

    pub fn teb(&self) -> u32 {
        self.config.runtime_base
    }
    pub fn array(&self) -> u32 {
        self.config.runtime_base + PAGE_SIZE
    }
    pub fn return_pc(&self) -> u32 {
        self.config.runtime_base + (1 + self.array_pages) * PAGE_SIZE
    }
    pub fn stack_end(&self) -> u32 {
        self.config
            .stack_base
            .wrapping_add(self.config.stack_pages * PAGE_SIZE)
    }

    pub fn reservations(&self) -> [(u64, u64); 2] {
        [
            (
                u64::from(self.config.runtime_base),
                u64::from(self.config.runtime_base) + u64::from(TLS_POOL_BYTES),
            ),
            (
                u64::from(self.config.stack_base),
                u64::from(self.config.stack_base)
                    + u64::from(self.config.stack_pages) * u64::from(PAGE_SIZE),
            ),
        ]
    }

    pub fn stage_runtime(&self, plan: &mut LoadPlan32) -> Result<(), ModuleError32> {
        let mut teb = [0; PAGE_SIZE as usize];
        teb[4..8].copy_from_slice(&self.stack_end().to_le_bytes());
        teb[8..12].copy_from_slice(&self.config.stack_base.to_le_bytes());
        teb[0x18..0x1c].copy_from_slice(&self.teb().to_le_bytes());
        teb[0x2c..0x30].copy_from_slice(&self.array().to_le_bytes());
        teb[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        plan.add_page(GuestAddress(self.teb()), Permissions::READ_WRITE, &teb)?;
        for page in 0..self.array_pages {
            plan.add_page(
                GuestAddress(self.array() + page * PAGE_SIZE),
                Permissions::READ_WRITE,
                &[],
            )?;
        }
        plan.add_page(
            GuestAddress(self.return_pc()),
            Permissions::READ_EXECUTE,
            &[0x0f, 0x0b],
        )?;
        for page in 0..self.config.stack_pages {
            plan.add_page(
                GuestAddress(self.config.stack_base + page * PAGE_SIZE),
                Permissions::READ_WRITE,
                &[],
            )?;
        }
        Ok(())
    }

    pub fn stage_modules(
        &self,
        plan: &mut LoadPlan32,
        memory: &AddressSpace,
        initial: bool,
    ) -> Result<(Vec<TlsAllocation>, Vec<WordWrite32>), ModuleError32> {
        let count = plan
            .additions()
            .iter()
            .filter(|module| module.tls().is_some())
            .count();
        let mut allocations: Vec<TlsAllocation> = Vec::new();
        let mut writes = Vec::new();
        allocations
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        writes
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        let mut next_address = self.config.runtime_base + (2 + self.array_pages) * PAGE_SIZE;
        for module_index in 0..plan.additions().len() {
            let module = &plan.additions()[module_index];
            let Some(tls) = module.tls() else {
                continue;
            };
            let size = u32::try_from(tls.template.len())
                .map_err(|_| ModuleError32::Capacity)?
                .checked_add(tls.zero_fill)
                .ok_or(ModuleError32::Capacity)?;
            let pages = size.max(1).div_ceil(PAGE_SIZE);
            let slot = (0..self.slots)
                .find(|slot| {
                    !self
                        .allocations
                        .iter()
                        .chain(allocations.iter())
                        .any(|allocation| allocation.slot == *slot)
                })
                .ok_or(ModuleError32::Capacity)?;
            let lower = self.config.runtime_base + (2 + self.array_pages) * PAGE_SIZE;
            let upper = u64::from(self.config.runtime_base) + u64::from(TLS_POOL_BYTES);
            let range = loop {
                if u64::from(next_address) + u64::from(pages) * u64::from(PAGE_SIZE) > upper {
                    return Err(ModuleError32::Capacity);
                }
                let bounds = PageRange::new(
                    GuestAddress(next_address.max(lower)),
                    ((upper - u64::from(next_address)) / u64::from(PAGE_SIZE)) as u32,
                )?;
                let candidate =
                    memory.find_free_range(bounds, pages, PAGE_SIZE.max(tls.alignment))?;
                let end = (candidate.first + candidate.count) as u64 * u64::from(PAGE_SIZE);
                if let Some(overlap) = allocations.iter().find(|allocation| {
                    candidate.first < allocation.range.first + allocation.range.count
                        && allocation.range.first < candidate.first + candidate.count
                }) {
                    next_address = ((overlap.range.first + overlap.range.count) as u64
                        * u64::from(PAGE_SIZE)) as u32;
                    continue;
                }
                next_address = end as u32;
                break candidate;
            };
            let block = range.first as u32 * PAGE_SIZE;
            let id = module.id();
            let index_address = tls.index_address;
            let template = tls.template.clone();
            plan.stage_write(index_address, &slot.to_le_bytes())?;
            for page in 0..pages {
                let offset = page as usize * PAGE_SIZE as usize;
                let bytes = template
                    .get(offset..template.len().min(offset + PAGE_SIZE as usize))
                    .unwrap_or(&[]);
                plan.add_page(
                    GuestAddress(block + page * PAGE_SIZE),
                    Permissions::READ_WRITE,
                    bytes,
                )?;
            }
            if initial {
                plan.stage_write(GuestAddress(self.array() + slot * 4), &block.to_le_bytes())?;
            } else {
                writes.push(WordWrite32 {
                    address: GuestAddress(self.array() + slot * 4),
                    value: block,
                });
            }
            allocations.push(TlsAllocation {
                module: id,
                slot,
                range,
            });
        }
        Ok((allocations, writes))
    }

    pub fn runtime_pages(&self) -> u32 {
        2 + self.array_pages
    }

    pub fn clear_writes(&self, ids: &[ModuleId32]) -> Result<Vec<WordWrite32>, ModuleError32> {
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(self.allocations.len())
            .map_err(|_| ModuleError32::Allocation)?;
        writes.extend(
            self.allocations
                .iter()
                .filter(|allocation| ids.contains(&allocation.module))
                .map(|allocation| WordWrite32 {
                    address: GuestAddress(self.array() + allocation.slot * 4),
                    value: 0,
                }),
        );
        writes.sort_unstable_by_key(|write| write.address.0);
        Ok(writes)
    }
}
