use super::super::{CapturedModulePaths32, ModuleRuntimeConfig32, tls::InitialThreadTls};
use crate::{
    loader::{ImportSymbol32, LoadPlan32, ModuleError32},
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError},
};
use std::cell::RefCell;

pub(in crate::process) enum ModulePlanProbe32<T = LoadPlan32> {
    Ready(T),
    Missing(String),
    Manifest(String),
}

enum PlanNeed32 {
    Bytes(String),
    Manifest(String),
}

fn probe_result<T>(
    result: Result<T, HostError>,
    need: RefCell<Option<PlanNeed32>>,
) -> Result<ModulePlanProbe32<T>, HostError> {
    match (result, need.into_inner()) {
        (Ok(plan), _) => Ok(ModulePlanProbe32::Ready(plan)),
        (Err(HostError::Module(ModuleError32::MissingModule)), Some(PlanNeed32::Bytes(name))) => {
            Ok(ModulePlanProbe32::Missing(name))
        }
        (Err(HostError::Module(ModuleError32::Busy)), Some(PlanNeed32::Manifest(name))) => {
            Ok(ModulePlanProbe32::Manifest(name))
        }
        (Err(error), _) => Err(error),
    }
}

impl EngineInstance {
    fn resolve_planned_module_import(
        &self,
        importer: &str,
        dependency: &str,
        paths: &CapturedModulePaths32,
        need: &RefCell<Option<PlanNeed32>>,
    ) -> Result<String, ModuleError32> {
        if !self.modules.catalog.is_system(importer)
            && self.modules.loaded.find(importer).is_none()
            && !paths.has_importer_activation(importer)
        {
            *need.borrow_mut() = Some(PlanNeed32::Manifest(importer.to_owned()));
            return Err(ModuleError32::Busy);
        }
        self.resolve_module_import_captured(importer, dependency, paths)
            .inspect(|identity| {
                if !self.modules.catalog.contains(identity)
                    && self.modules.loaded.find(identity).is_none()
                {
                    *need.borrow_mut() = Some(PlanNeed32::Bytes(identity.clone()));
                }
            })
            .map_err(|error| match error {
                HostError::Module(error) => error,
                _ => ModuleError32::InvalidName,
            })
    }

    fn captured_module_reservations(
        &self,
        config: Option<ModuleRuntimeConfig32>,
    ) -> Result<Vec<PageRange>, HostError> {
        let mut reservations = self.guest_address_reservations()?;
        if let Some(config) = config {
            let slots = self
                .modules
                .catalog
                .limits()
                .modules
                .min(self.memory()?.capacity_pages() as usize) as u32;
            let thread = InitialThreadTls::new(config, slots).map_err(HostError::Module)?;
            for (start, end) in thread.reservations() {
                let range = PageRange::new(
                    GuestAddress(start as u32),
                    ((end - start) / u64::from(crate::memory::PAGE_SIZE)) as u32,
                )
                .map_err(HostError::Memory)?;
                if self
                    .provider_guest_arenas()
                    .any(|old| crate::process::memory::reservations::overlaps(old, range))
                {
                    return Err(HostError::Module(ModuleError32::AddressConflict));
                }
                reservations.push(range);
            }
        }
        Ok(reservations)
    }

    pub(in crate::process) fn probe_captured_module_headers(
        &self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
        _paths: &CapturedModulePaths32,
    ) -> Result<ModulePlanProbe32, HostError> {
        if !self.modules.catalog.contains(name) && self.modules.loaded.find(name).is_none() {
            return Ok(ModulePlanProbe32::Missing(name.to_owned()));
        }
        let reservations = self.captured_module_reservations(config)?;
        self.modules
            .catalog
            .plan_load_headers(
                &self.modules.loaded,
                self.memory()?,
                name,
                actual_base,
                &reservations,
                if config.is_some() && self.modules.windows_runtime {
                    &["ntdll.dll"]
                } else {
                    &[]
                },
            )
            .map(ModulePlanProbe32::Ready)
            .map_err(HostError::Module)
    }

    pub(in crate::process) fn probe_captured_module_load(
        &self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
        paths: &CapturedModulePaths32,
    ) -> Result<ModulePlanProbe32, HostError> {
        if !self.modules.catalog.contains(name) && self.modules.loaded.find(name).is_none() {
            return Ok(ModulePlanProbe32::Missing(name.to_owned()));
        }
        let missing = RefCell::new(None);
        let result =
            self.plan_captured_module_load_with_missing(name, actual_base, config, paths, &missing);
        probe_result(result, missing)
    }

    pub(in crate::process) fn probe_captured_module_export(
        &self,
        handle: u32,
        symbol: &ImportSymbol32,
        paths: &CapturedModulePaths32,
    ) -> Result<ModulePlanProbe32<(LoadPlan32, u32)>, HostError> {
        let id = self
            .modules
            .loaded
            .iter()
            .find(|module| module.base() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?
            .id();
        let reservations = self.guest_address_reservations()?;
        let missing = RefCell::new(None);
        let result = self.modules.catalog.plan_export_live_with_resolver(
            &self.modules.loaded,
            self.memory()?,
            id,
            symbol,
            &reservations,
            &|importer, dependency| {
                self.resolve_planned_module_import(importer, dependency, paths, &missing)
            },
        );
        probe_result(
            result
                .map(|(plan, resolved)| (plan, resolved.address().0))
                .map_err(HostError::Module),
            missing,
        )
    }
    fn plan_captured_module_load_with_missing(
        &self,
        name: &str,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
        paths: &CapturedModulePaths32,
        missing: &RefCell<Option<PlanNeed32>>,
    ) -> Result<crate::loader::LoadPlan32, HostError> {
        let reservations = self.captured_module_reservations(config)?;
        self.modules
            .catalog
            .plan_load_with_resolver(
                &self.modules.loaded,
                self.memory()?,
                name,
                actual_base,
                &reservations,
                if config.is_some() && self.modules.windows_runtime {
                    &["ntdll.dll"]
                } else {
                    &[]
                },
                &|importer, dependency| {
                    self.resolve_planned_module_import(importer, dependency, paths, missing)
                },
            )
            .map_err(HostError::Module)
    }
}
