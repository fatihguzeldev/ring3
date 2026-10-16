use super::{CapturedModuleRequest32, PendingModuleLoad32, acquisition::ModuleFileInput32};
use crate::{
    files::FileIo32,
    loader::{DataFilePlan32, LoadPlan32, ModuleError32},
    process::{
        EngineInstance, HostError,
        loading::modules::{
            ModulePlanProbe32, ModuleRuntimeConfig32, initialization::LoadCompletion,
        },
        windows::activation::modules::{ModuleActivationSnapshot32, ModuleActivationWork32},
    },
};

pub(super) enum ReadyModuleSource32 {
    Image {
        plan: LoadPlan32,
        config: Option<ModuleRuntimeConfig32>,
        completion: LoadCompletion,
    },
    DataFile(DataFilePlan32),
}

impl EngineInstance {
    pub(super) fn poll_module_source(
        &mut self,
        source: &mut PendingModuleLoad32,
    ) -> Result<FileIo32<ReadyModuleSource32>, HostError> {
        loop {
            if let Some(input) = &mut source.input {
                source
                    .leases
                    .try_reserve(1)
                    .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
                match input.poll(self)? {
                    FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                    FileIo32::Ready(()) => {
                        let input = source.input.take().unwrap();
                        source.leases.push(super::leases::ModuleSourceLease32 {
                            identity: input.name,
                            lease: input.lease,
                        });
                    }
                }
            }
            if let CapturedModuleRequest32::Load {
                name,
                actual_base,
                config: None,
                no_resolve,
                ..
            } = &source.request
            {
                if !self.modules.catalog.contains(name) && self.modules.loaded.find(name).is_none()
                {
                    source.input = Some(ModuleFileInput32::new(self, name.clone())?);
                    continue;
                }
                let dll = match self.modules.loaded.find(name) {
                    Some(module) => module.is_dll(),
                    None => self
                        .modules
                        .catalog
                        .is_dll(name)
                        .map_err(HostError::Module)?,
                };
                if *no_resolve || !dll {
                    let plan = self
                        .modules
                        .catalog
                        .plan_load_image(
                            &self.modules.loaded,
                            self.memory()?,
                            name,
                            *actual_base,
                            &self.guest_address_reservations()?,
                        )
                        .map_err(HostError::Module)?;
                    self.retain_module_plan_sources(source, &plan)?;
                    return Ok(FileIo32::Ready(ReadyModuleSource32::Image {
                        plan,
                        config: None,
                        completion: LoadCompletion::Library,
                    }));
                }
            }
            if let CapturedModuleRequest32::DataFile { name, layout, .. } = &source.request {
                let name = name.clone();
                let layout = *layout;
                if !self.modules.catalog.contains(&name) {
                    source.input = Some(ModuleFileInput32::new(self, name)?);
                    continue;
                }
                self.retain_module_source(source, &name)?;
                let reserved = self.guest_address_reservations()?;
                let plan = self
                    .modules
                    .catalog
                    .plan_datafile(
                        &self.modules.loaded,
                        self.memory()?,
                        &name,
                        layout,
                        &reserved,
                    )
                    .map_err(HostError::Module)?;
                return Ok(FileIo32::Ready(ReadyModuleSource32::DataFile(plan)));
            }
            if let Some(name) = source.manifest.as_ref() {
                let (base, config) = match &source.request {
                    CapturedModuleRequest32::Load {
                        name: root,
                        actual_base,
                        config,
                        ..
                    } if root == name => (*actual_base, *config),
                    _ => (None, None),
                };
                let plan =
                    match self.probe_captured_module_headers(name, base, config, source.paths())? {
                        ModulePlanProbe32::Ready(plan) => plan,
                        ModulePlanProbe32::Missing(name) => {
                            source.input = Some(ModuleFileInput32::new(self, name)?);
                            continue;
                        }
                        ModulePlanProbe32::Manifest(_) => return Err(HostError::Infrastructure),
                    };
                self.retain_module_plan_sources(source, &plan)?;
                self.enqueue_module_manifests(source, &plan, config.is_some())?;
                source.manifest = None;
            }
            match self.poll_module_activation_work(source.works.as_mut().unwrap())? {
                FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                FileIo32::Ready(()) => {}
            }
            self.bind_prepared_module_manifests(source)?;
            let (probe, config, completion) = match &source.request {
                CapturedModuleRequest32::Load {
                    name,
                    actual_base,
                    config,
                    paths,
                    ..
                } => (
                    self.probe_captured_module_load(name, *actual_base, *config, paths)?,
                    *config,
                    LoadCompletion::Library,
                ),
                CapturedModuleRequest32::Export {
                    handle,
                    symbol,
                    paths,
                } => match self.probe_captured_module_export(*handle, symbol, paths)? {
                    ModulePlanProbe32::Ready((plan, address)) => (
                        ModulePlanProbe32::Ready(plan),
                        None,
                        LoadCompletion::Export(address),
                    ),
                    ModulePlanProbe32::Missing(name) => (
                        ModulePlanProbe32::Missing(name),
                        None,
                        LoadCompletion::Library,
                    ),
                    ModulePlanProbe32::Manifest(name) => (
                        ModulePlanProbe32::Manifest(name),
                        None,
                        LoadCompletion::Library,
                    ),
                },
                CapturedModuleRequest32::DataFile { .. } => unreachable!(),
            };
            let plan = match probe {
                ModulePlanProbe32::Ready(plan) => plan,
                ModulePlanProbe32::Missing(name) => {
                    source.input = Some(ModuleFileInput32::new(self, name)?);
                    continue;
                }
                ModulePlanProbe32::Manifest(name) => {
                    source.manifest = Some(name);
                    continue;
                }
            };
            self.retain_module_plan_sources(source, &plan)?;
            if self.enqueue_module_manifests(source, &plan, config.is_some())? {
                continue;
            }
            return Ok(FileIo32::Ready(ReadyModuleSource32::Image {
                plan,
                config,
                completion,
            }));
        }
    }

    fn enqueue_module_manifests(
        &self,
        source: &mut PendingModuleLoad32,
        plan: &LoadPlan32,
        initial: bool,
    ) -> Result<bool, HostError> {
        if source.application_directory.is_none() {
            let path = if initial {
                let root = plan
                    .additions()
                    .iter()
                    .find(|module| module.id() == plan.root())
                    .ok_or(HostError::Infrastructure)?;
                self.planned_module_file_name(root, Some(root))?
            } else {
                self.virtual_module_file_name_units(0)?
            };
            let mut directory = crate::files::path::parent(&path)
                .map_err(HostError::File)?
                .to_vec();
            if directory.last() != Some(&92) {
                directory.push(92);
            }
            source.application_directory = Some(directory);
        }
        let identities: Vec<_> = plan
            .additions()
            .iter()
            .filter(|module| {
                !module.is_system()
                    && !source.paths().has_importer_activation(module.identity())
                    && !source
                        .header_identities
                        .iter()
                        .any(|identity| identity == module.identity())
            })
            .map(|module| module.identity().to_owned())
            .collect();
        if identities.is_empty() {
            return Ok(false);
        }
        let additional = self.module_activation_work(
            plan,
            initial,
            source.works.as_ref().unwrap(),
            source.application_directory.as_deref().unwrap(),
        )?;
        source
            .header_identities
            .try_reserve(identities.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        let works = source.works.as_mut().unwrap();
        works
            .try_reserve(additional.len())
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        source.header_identities.extend(identities);
        works.extend(additional);
        Ok(true)
    }

    fn bind_prepared_module_manifests(
        &self,
        source: &mut PendingModuleLoad32,
    ) -> Result<(), HostError> {
        let works = source.works.as_ref().unwrap();
        let prepared_default = works
            .iter()
            .find(|work| work.process_default())
            .and_then(ModuleActivationWork32::graph);
        let default = prepared_default
            .clone()
            .or_else(|| source.default_graph.clone());
        let overlays: Vec<_> = source
            .header_identities
            .iter()
            .map(|identity| {
                let graph = works
                    .iter()
                    .find(|work| work.identity() == identity)
                    .and_then(ModuleActivationWork32::graph);
                let fallback = default
                    .as_ref()
                    .filter(|default| {
                        graph
                            .as_ref()
                            .is_none_or(|graph| !std::sync::Arc::ptr_eq(default, graph))
                    })
                    .cloned();
                (
                    identity.clone(),
                    ModuleActivationSnapshot32::from_optional_graph(graph, fallback),
                )
            })
            .collect();
        let paths = source.paths_mut();
        if let Some(graph) = prepared_default {
            paths.set_process_default_activation(ModuleActivationSnapshot32::from_graph(
                graph, None,
            ));
        }
        for (identity, snapshot) in overlays {
            paths.set_importer_activation(identity, snapshot)?;
        }
        Ok(())
    }
}

impl PendingModuleLoad32 {
    fn paths(&self) -> &super::super::CapturedModulePaths32 {
        match &self.request {
            CapturedModuleRequest32::Load { paths, .. }
            | CapturedModuleRequest32::Export { paths, .. }
            | CapturedModuleRequest32::DataFile { paths, .. } => paths,
        }
    }
    fn paths_mut(&mut self) -> &mut super::super::CapturedModulePaths32 {
        match &mut self.request {
            CapturedModuleRequest32::Load { paths, .. }
            | CapturedModuleRequest32::Export { paths, .. }
            | CapturedModuleRequest32::DataFile { paths, .. } => paths,
        }
    }
}
