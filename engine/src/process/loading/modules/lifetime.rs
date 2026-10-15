use super::ModuleProcess32;
use crate::{
    loader::{ImportSymbol32, ModuleError32, ModuleId32, ResolvedExport32, SystemModule32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
};

pub(super) struct ModuleReference {
    pub id: ModuleId32,
    pub explicit: u32,
    pub pinned: bool,
}

impl ModuleProcess32 {
    pub(super) fn reference(&self, id: ModuleId32) -> Result<&ModuleReference, ModuleError32> {
        self.references
            .iter()
            .find(|reference| reference.id == id)
            .ok_or(ModuleError32::InvalidHandle)
    }

    pub(super) fn retain(&mut self, id: ModuleId32, initial: bool) -> Result<(), ModuleError32> {
        let reference = self
            .references
            .iter_mut()
            .find(|reference| reference.id == id)
            .ok_or(ModuleError32::InvalidHandle)?;
        if initial {
            reference.pinned = true;
        } else {
            reference.explicit = reference
                .explicit
                .checked_add(1)
                .ok_or(ModuleError32::ReferenceExhausted)?;
        }
        Ok(())
    }

    pub(super) fn unreachable_after_release(
        &self,
        root: ModuleId32,
    ) -> Result<Vec<ModuleId32>, ModuleError32> {
        let reference = self.reference(root)?;
        if reference.pinned || reference.explicit == 0 {
            return Err(ModuleError32::InvalidHandle);
        }
        let count = self.loaded.iter().len();
        let mut reachable = Vec::new();
        reachable
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        for reference in &self.references {
            let count = reference.explicit - u32::from(reference.id == root);
            if reference.pinned || count != 0 {
                reachable.push(reference.id);
            }
        }
        let mut cursor = 0;
        while cursor < reachable.len() {
            let module = self
                .loaded
                .get(reachable[cursor])
                .ok_or(ModuleError32::InvalidHandle)?;
            for dependency in module.dependencies() {
                if !reachable.contains(dependency) {
                    reachable.push(*dependency);
                }
            }
            cursor += 1;
        }
        let mut unload = Vec::new();
        unload
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        unload.extend(
            self.loaded
                .iter()
                .filter(|module| !reachable.contains(&module.id()))
                .map(|module| module.id()),
        );
        Ok(unload)
    }

    pub(super) fn rollback_ids(
        &self,
        new_ids: &[ModuleId32],
        initial: bool,
        introduced_edges: &[(ModuleId32, ModuleId32)],
    ) -> Result<Vec<ModuleId32>, ModuleError32> {
        let count = self.loaded.iter().len();
        let mut reachable = Vec::new();
        reachable
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        if !initial {
            reachable.extend(
                self.references
                    .iter()
                    .filter(|reference| reference.pinned || reference.explicit != 0)
                    .map(|reference| reference.id),
            );
            let mut cursor = 0;
            while cursor < reachable.len() {
                let module = self
                    .loaded
                    .get(reachable[cursor])
                    .ok_or(ModuleError32::InvalidHandle)?;
                for dependency in module.dependencies() {
                    if !introduced_edges.contains(&(module.id(), *dependency))
                        && !reachable.contains(dependency)
                    {
                        reachable.push(*dependency);
                    }
                }
                cursor += 1;
            }
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        ids.extend(
            self.loaded
                .iter()
                .filter(|module| {
                    (initial || new_ids.contains(&module.id())) && !reachable.contains(&module.id())
                })
                .map(|module| module.id()),
        );
        Ok(ids)
    }

    pub(super) fn remove_records(&mut self, ids: &[ModuleId32]) {
        self.references
            .retain(|reference| !ids.contains(&reference.id));
        self.attached.retain(|id| !ids.contains(id));
        if let Some(thread) = self.thread.as_mut() {
            thread
                .allocations
                .retain(|allocation| !ids.contains(&allocation.module));
        }
    }
}

impl EngineInstance {
    pub(in crate::process) fn has_active_module_operation(&self) -> bool {
        !self.modules.operations.is_empty()
    }

    pub fn configure_module_limits(
        &mut self,
        limits: crate::loader::ModuleLimits32,
    ) -> Result<(), HostError> {
        self.memory()?;
        if !self.modules.catalog.is_empty()
            || self.modules.input.is_some()
            || self.modules.loaded.iter().len() != 0
            || !self.modules.operations.is_empty()
            || self.pending_call.is_some()
            || self.callback.is_some()
            || self.image.is_some()
        {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.modules.catalog =
            crate::loader::ModuleCatalog32::with_limits(limits).map_err(HostError::Module)?;
        Ok(())
    }

    pub fn register_guest_module(&mut self, name: &str, bytes: &[u8]) -> Result<(), HostError> {
        self.memory()?;
        if !self.modules.operations.is_empty()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.modules
            .catalog
            .add_guest(name, bytes)
            .map_err(HostError::Module)
    }

    pub fn register_system_module(&mut self, module: SystemModule32) -> Result<(), HostError> {
        self.memory()?;
        if !self.modules.operations.is_empty()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.modules
            .catalog
            .add_system(module)
            .map_err(HostError::Module)
    }

    pub fn module_teb_address(&self) -> Option<u32> {
        self.modules.thread.as_ref().map(|thread| thread.teb())
    }

    pub fn module_handle(&self, name: Option<&str>) -> Result<Option<u32>, HostError> {
        self.memory()?;
        let module = match name {
            Some(name) => {
                crate::loader::normalize_module_name(name).map_err(HostError::Module)?;
                self.modules.loaded.find(name)
            }
            None => self
                .modules
                .loaded
                .root()
                .and_then(|id| self.modules.loaded.get(id)),
        };
        Ok(module.map(|module| module.base()))
    }

    pub fn module_export(
        &self,
        handle: u32,
        symbol: &ImportSymbol32,
    ) -> Result<ResolvedExport32, HostError> {
        self.memory()?;
        let id = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        self.modules
            .loaded
            .resolve_export_live(self.memory()?, id, symbol)
            .map_err(HostError::Module)
    }

    pub fn module_gates(&self) -> Result<Vec<crate::cpu::dbt::GateSpec>, HostError> {
        self.memory()?;
        let mut gates = Vec::new();
        let count: usize = self
            .modules
            .loaded
            .iter()
            .map(|module| module.gates().len())
            .sum();
        gates
            .try_reserve_exact(count + usize::from(self.modules.thread.is_some()))
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        for module in self.modules.loaded.iter() {
            gates.extend_from_slice(module.gates());
        }
        if let Some(thread) = &self.modules.thread {
            gates.push(crate::cpu::dbt::GateSpec {
                entry: GuestAddress(thread.return_pc()),
                id: super::initialization::INITIALIZER_RETURN_ID,
            });
        }
        Ok(gates)
    }

    pub(in crate::process) fn module_image_contains_range(&self, range: PageRange) -> bool {
        let start = range.first as u64 * u64::from(crate::memory::PAGE_SIZE);
        let end = start + range.count as u64 * u64::from(crate::memory::PAGE_SIZE);
        self.modules.loaded.iter().any(|module| {
            start >= u64::from(module.base())
                && end <= u64::from(module.base()) + u64::from(module.image_size())
        })
    }

    pub(in crate::process) fn file_mapping_reservations(
        &self,
    ) -> Result<Vec<PageRange>, HostError> {
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(
                self.virtual_allocations.len() + self.modules.loaded.iter().count() + 3,
            )
            .map_err(|_| HostError::Memory(crate::memory::MemoryError::Allocation))?;
        ranges.extend_from_slice(&self.virtual_allocations);
        for module in self.modules.loaded.iter() {
            ranges.push(
                PageRange::new(
                    crate::memory::GuestAddress(module.base()),
                    module.image_size().div_ceil(crate::memory::PAGE_SIZE),
                )
                .map_err(HostError::Memory)?,
            );
        }
        if let Some(image) = self.image {
            ranges.push(
                PageRange::new(
                    crate::memory::GuestAddress(image.image_base),
                    image.image_size.div_ceil(crate::memory::PAGE_SIZE),
                )
                .map_err(HostError::Memory)?,
            );
        }
        if let Some(thread) = &self.modules.thread {
            for (start, end) in thread.reservations() {
                ranges.push(
                    PageRange::new(
                        crate::memory::GuestAddress(start as u32),
                        ((end - start) / u64::from(crate::memory::PAGE_SIZE)) as u32,
                    )
                    .map_err(HostError::Memory)?,
                );
            }
        }
        Ok(ranges)
    }
    pub(in crate::process) fn module_reservation_end(&self, range: PageRange) -> Option<u64> {
        let start = range.first as u64 * u64::from(crate::memory::PAGE_SIZE);
        let end = start + range.count as u64 * u64::from(crate::memory::PAGE_SIZE);
        let mut overlapping_end = None;
        for module in self.modules.loaded.iter() {
            let module_start = u64::from(module.base());
            let module_end = module_start + u64::from(module.image_size());
            if start < module_end && module_start < end {
                overlapping_end = Some(overlapping_end.unwrap_or(0).max(module_end));
            }
        }
        if let Some(thread) = &self.modules.thread {
            for (reserved_start, reserved_end) in thread.reservations() {
                if start < reserved_end && reserved_start < end {
                    overlapping_end = Some(overlapping_end.unwrap_or(0).max(reserved_end));
                }
            }
        }
        overlapping_end
    }
}
