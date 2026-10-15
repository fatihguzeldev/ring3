use super::{GuestComError32, GuestComObjects32, InterfaceLayout32};
use crate::memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions};
use crate::windows::com::ComRegistry32;

impl GuestComObjects32 {
    pub(crate) fn new(
        bounds: PageRange,
        max_objects: usize,
        max_interfaces: usize,
        max_vtables: usize,
    ) -> Result<Self, GuestComError32> {
        if bounds.first == 0 || !(1..=65_536).contains(&max_vtables) {
            return Err(GuestComError32::InvalidLimits);
        }
        let registry = ComRegistry32::new(max_objects, max_interfaces)?;
        let mut vtables = Vec::new();
        vtables
            .try_reserve_exact(max_vtables)
            .map_err(|_| GuestComError32::Allocation)?;
        let mut allocations = Vec::new();
        allocations
            .try_reserve_exact(max_objects)
            .map_err(|_| GuestComError32::Allocation)?;
        Ok(Self {
            registry,
            bounds,
            next_page: bounds.first,
            memory_identity: None,
            vtable_limit: max_vtables,
            vtables,
            allocations,
        })
    }

    pub(crate) fn live_objects(&self) -> usize {
        self.registry.live_objects()
    }

    pub(crate) fn retained_pages(&self) -> usize {
        self.vtables.len() + self.allocations.len()
    }

    pub(crate) fn high_water_address(&self) -> u64 {
        self.next_page as u64 * u64::from(PAGE_SIZE)
    }

    pub(super) fn reserve_range(
        &self,
        memory: &AddressSpace,
        count: u32,
    ) -> Result<PageRange, GuestComError32> {
        self.check_memory(memory)?;
        let end = self.bounds.first + self.bounds.count;
        if self.next_page >= end {
            return Err(GuestComError32::Capacity);
        }
        let bounds = PageRange::new(
            GuestAddress((self.next_page as u64 * u64::from(PAGE_SIZE)) as u32),
            (end - self.next_page) as u32,
        )?;
        Ok(memory.find_free_range(bounds, count, PAGE_SIZE)?)
    }

    pub(super) fn check_memory(&self, memory: &AddressSpace) -> Result<(), GuestComError32> {
        if self
            .memory_identity
            .is_some_and(|identity| identity != memory.identity())
        {
            return Err(GuestComError32::StaleMapping);
        }
        Ok(())
    }

    pub(super) fn check_mapping(
        &self,
        memory: &AddressSpace,
        address: GuestAddress,
        version: u64,
        permissions: Permissions,
    ) -> Result<(), GuestComError32> {
        self.check_memory(memory)?;
        if memory.mapping_version_at(address)? != version
            || memory.permissions_at(address)? != permissions
        {
            return Err(GuestComError32::StaleMapping);
        }
        Ok(())
    }

    pub(super) fn cached_vtable(&self, layout: InterfaceLayout32<'_>) -> Option<usize> {
        self.vtables
            .iter()
            .position(|table| table.iid == layout.iid && table.methods.as_ref() == layout.methods)
    }
}

pub(super) fn validate_layouts(layouts: &[InterfaceLayout32<'_>]) -> Result<(), GuestComError32> {
    if layouts.is_empty() || layouts.len() > 64 {
        return Err(GuestComError32::InvalidLayout);
    }
    for (index, layout) in layouts.iter().enumerate() {
        if !(3..=1024).contains(&layout.methods.len())
            || layout.methods.iter().any(|address| address.0 == 0)
            || layouts[..index]
                .iter()
                .any(|earlier| earlier.iid == layout.iid)
        {
            return Err(GuestComError32::InvalidLayout);
        }
    }
    Ok(())
}
