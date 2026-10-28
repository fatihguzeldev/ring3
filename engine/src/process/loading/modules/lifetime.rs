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

pub(in crate::process) struct PreparedModuleReference32 {
    id: ModuleId32,
    explicit: u32,
    pinned: bool,
    next_explicit: u32,
    next_pinned: bool,
}

fn native_reference_counts(
    explicit: u32,
    pinned: bool,
    pin: bool,
) -> Result<(u32, bool), ModuleError32> {
    if pinned || pin {
        Ok((explicit, true))
    } else {
        Ok((
            explicit
                .checked_add(1)
                .ok_or(ModuleError32::ReferenceExhausted)?,
            false,
        ))
    }
}

impl EngineInstance {
    pub(in crate::process) fn prepare_native_module_reference(
        &self,
        base: u32,
        pin: bool,
    ) -> Result<Option<PreparedModuleReference32>, HostError> {
        let Some(module) = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == base)
        else {
            return Ok(None);
        };
        let reference = self
            .modules
            .reference(module.id())
            .map_err(HostError::Module)?;
        let (next_explicit, next_pinned) =
            native_reference_counts(reference.explicit, reference.pinned, pin)
                .map_err(HostError::Module)?;
        Ok(Some(PreparedModuleReference32 {
            id: reference.id,
            explicit: reference.explicit,
            pinned: reference.pinned,
            next_explicit,
            next_pinned,
        }))
    }
    pub(in crate::process) fn prepare_module_handle_reference(
        &self,
        name: Option<&str>,
        address: Option<GuestAddress>,
        flags: u32,
    ) -> Result<Option<(u32, PreparedModuleReference32)>, HostError> {
        let module = if let Some(address) = address {
            self.modules.loaded.containing(address)
        } else if let Some(name) = name {
            self.modules.loaded.find(name)
        } else {
            self.modules
                .loaded
                .root()
                .and_then(|id| self.modules.loaded.get(id))
        };
        let Some(module) = module else {
            return Ok(None);
        };
        let reference = self
            .modules
            .reference(module.id())
            .map_err(HostError::Module)?;
        let next_explicit = if flags & 2 == 0 {
            reference
                .explicit
                .checked_add(1)
                .ok_or(HostError::Module(ModuleError32::ReferenceExhausted))?
        } else {
            reference.explicit
        };
        Ok(Some((
            module.base(),
            PreparedModuleReference32 {
                id: reference.id,
                explicit: reference.explicit,
                pinned: reference.pinned,
                next_explicit,
                next_pinned: reference.pinned || flags & 1 != 0,
            },
        )))
    }

    pub(in crate::process) fn commit_module_handle_reference(
        &mut self,
        prepared: PreparedModuleReference32,
    ) {
        let reference = self
            .modules
            .references
            .iter_mut()
            .find(|reference| reference.id == prepared.id)
            .expect("module reference was prepared before output publication");
        assert_eq!(
            (reference.explicit, reference.pinned),
            (prepared.explicit, prepared.pinned)
        );
        reference.explicit = prepared.next_explicit;
        reference.pinned = prepared.next_pinned;
    }
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
        self.thread_notifications_disabled
            .retain(|id| !ids.contains(id));
        if let Some(thread) = self.thread.as_mut() {
            thread
                .allocations
                .retain(|allocation| !ids.contains(&allocation.module));
        }
    }
}

impl EngineInstance {
    pub(in crate::process) fn enable_windows_runtime_profile(&mut self) {
        self.modules.windows_runtime = true;
    }
    pub(in crate::process) fn has_active_module_operation(&self) -> bool {
        !self.modules.operations.is_empty() || self.modules.source.is_some()
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_module_ready(&self) -> bool {
        self.modules.input.is_none()
            && self.modules.source.is_none()
            && (self.modules.operations.is_empty()
                || self
                    .modules
                    .operations
                    .last()
                    .is_some_and(|operation| operation.active.is_some()))
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

    pub fn current_teb_address(&self) -> Option<u32> {
        self.threads
            .current_teb()
            .or_else(|| self.modules.thread.as_ref().map(|thread| thread.teb()))
    }

    pub fn module_teb_address(&self) -> Option<u32> {
        self.current_teb_address()
    }

    pub fn module_handle(&self, name: Option<&str>) -> Result<Option<u32>, HostError> {
        self.memory()?;
        let module = match name {
            Some(name) => {
                crate::loader::normalize_module_identity(name).map_err(HostError::Module)?;
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

    pub(in crate::process) fn loaded_windows_modules(
        &self,
    ) -> impl ExactSizeIterator<Item = &crate::loader::LoadedModule32> {
        self.modules.loaded.iter()
    }

    pub(in crate::process) fn module_gate_at(&self, pc: u32) -> Option<crate::cpu::dbt::GateSpec> {
        self.modules.gate_at(pc)
    }
}

impl super::ModuleProcess32 {
    pub(in crate::process) fn gate_at(&self, pc: u32) -> Option<crate::cpu::dbt::GateSpec> {
        self.loaded.gate(GuestAddress(pc)).or_else(|| {
            self.thread
                .as_ref()
                .filter(|thread| thread.return_pc() == pc)
                .map(|_| crate::cpu::dbt::GateSpec {
                    entry: GuestAddress(pc),
                    id: super::initialization::INITIALIZER_RETURN_ID,
                })
        })
    }
}

impl EngineInstance {
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

    pub(in crate::process) fn initial_thread_reservations(&self) -> Option<[(u64, u64); 2]> {
        self.modules
            .thread
            .as_ref()
            .map(|thread| thread.reservations())
    }
}

impl EngineInstance {
    pub(in crate::process) fn has_registered_module(&self, name: &str) -> bool {
        self.modules.catalog.contains(name)
    }
    pub(in crate::process) fn register_system_module_group(
        &mut self,
        modules: Vec<SystemModule32>,
    ) -> Result<(), HostError> {
        self.memory()?;
        if !self.modules.operations.is_empty()
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(HostError::Module(ModuleError32::Busy));
        }
        self.modules
            .catalog
            .add_system_group(modules)
            .map_err(HostError::Module)
    }
}

#[cfg(test)]
mod native_reference_tests {
    use super::*;
    #[test]
    fn native_pinning_is_idempotent_even_at_the_last_ordinary_reference() {
        assert_eq!(
            native_reference_counts(u32::MAX, false, true),
            Ok((u32::MAX, true))
        );
        assert_eq!(
            native_reference_counts(u32::MAX, true, false),
            Ok((u32::MAX, true))
        );
        assert_eq!(
            native_reference_counts(u32::MAX, true, true),
            Ok((u32::MAX, true))
        );
        assert_eq!(
            native_reference_counts(u32::MAX, false, false),
            Err(ModuleError32::ReferenceExhausted)
        );
        assert_eq!(native_reference_counts(7, false, false), Ok((8, false)));
    }
}
