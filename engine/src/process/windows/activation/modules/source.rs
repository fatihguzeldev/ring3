use super::super::{
    ActivationRuntime32,
    sources::{CreateSource32, CreateWork32},
};
use crate::{
    files::FileIo32,
    loader::{
        LoadPlan32, LoadedModule32, ParserLimits32, PeResources32, ResourceError32,
        ResourceIdentifier32, ResourceImageRead32,
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::activation::{ActivationConfig32, ActivationError32 as E},
};

pub(in crate::process) struct ModuleActivationWork32 {
    pub(super) identity: String,
    pub(super) process_default: bool,
    pub(super) work: CreateWork32,
    pub(super) absent: bool,
}
struct PlannedResource32<'a> {
    plan: &'a LoadPlan32,
    base: u32,
}
impl ResourceImageRead32 for PlannedResource32<'_> {
    fn read_image(&self, rva: u32, output: &mut [u8]) -> Result<(), ResourceError32> {
        let address = self
            .base
            .checked_add(rva)
            .ok_or(ResourceError32::Malformed)?;
        self.plan
            .read_staged(GuestAddress(address), output)
            .map_err(|_| ResourceError32::Malformed)
    }
}
impl EngineInstance {
    pub(in crate::process) fn ensure_module_activation_runtime(&mut self) -> Result<(), HostError> {
        if self.activation.is_none() {
            let config = ActivationConfig32::default();
            self.check_guest_reservation_admission(&[config.section_bounds])?;
            self.activation =
                Some(ActivationRuntime32::new(config).map_err(HostError::Activation)?);
        }
        Ok(())
    }
    pub(in crate::process) fn module_activation_work(
        &self,
        plan: &LoadPlan32,
        initial: bool,
        known: &[ModuleActivationWork32],
        application_directory: &[u16],
    ) -> Result<Vec<ModuleActivationWork32>, HostError> {
        let config = self
            .activation
            .as_ref()
            .ok_or(HostError::Infrastructure)?
            .config;
        let root = plan
            .additions()
            .iter()
            .find(|module| module.id() == plan.root())
            .or_else(|| {
                self.loaded_windows_modules()
                    .find(|module| module.id() == plan.root())
            })
            .ok_or(HostError::Infrastructure)?;
        let mut works = Vec::new();
        for module in plan.additions() {
            if known.iter().any(|work| work.identity == module.identity()) {
                continue;
            }
            if module.is_system() {
                continue;
            }
            let bytes = manifest_bytes(plan, module, config, self.current_thread_ui_language())?;
            if bytes.is_none() && self.files.is_none() {
                continue;
            }
            works
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
            let resource = ResourceIdentifier32::Id(if module.is_dll() { 2 } else { 1 });
            let path = self.planned_module_file_name(module, initial.then_some(root))?;
            let process_default = initial && module.id() == root.id();
            let work = if let Some(bytes) = bytes {
                CreateWork32::from_manifest(
                    &bytes,
                    path,
                    resource,
                    application_directory.to_vec(),
                    process_default,
                    self.current_thread_ui_language(),
                    config,
                )?
            } else {
                let mut source = path;
                source.extend(".manifest".encode_utf16());
                CreateWork32::new(CreateSource32 {
                    source: Some(source),
                    resource: None,
                    module: None,
                    language: self.current_thread_ui_language(),
                    assembly_directory: None,
                    application: Some(application_directory.to_vec()),
                    process_default,
                })
            };
            works.push(ModuleActivationWork32 {
                identity: module.identity().to_owned(),
                process_default: initial && module.id() == root.id(),
                work,
                absent: false,
            });
        }
        Ok(works)
    }
    pub(in crate::process) fn poll_module_activation_work(
        &mut self,
        works: &mut [ModuleActivationWork32],
    ) -> Result<FileIo32<()>, HostError> {
        let config = self
            .activation
            .as_ref()
            .ok_or(HostError::Infrastructure)?
            .config;
        for work in works {
            if work.absent {
                continue;
            }
            match work.work.poll(self, config) {
                Ok(FileIo32::Ready(())) => {}
                Ok(FileIo32::NeedBytes(request)) => return Ok(FileIo32::NeedBytes(request)),
                Err(HostError::File(
                    crate::files::FileError32::NotFound | crate::files::FileError32::PathNotFound,
                )) if work.work.source_cost == 0 => work.absent = true,
                Err(error) => return Err(error),
            }
        }
        Ok(FileIo32::Ready(()))
    }
}
fn manifest_bytes(
    plan: &LoadPlan32,
    module: &LoadedModule32,
    config: ActivationConfig32,
    language: u16,
) -> Result<Option<Vec<u8>>, HostError> {
    let Some(directory) = module.resource_directory() else {
        return Ok(None);
    };
    let source = PlannedResource32 {
        plan,
        base: module.base(),
    };
    let mut reader = PeResources32::from_image_source(
        &source,
        module.image_size(),
        directory,
        ParserLimits32 {
            metadata_entries: config.nodes as usize,
            string_bytes: config.manifest_bytes as usize,
        },
    )
    .map_err(|error| HostError::Activation(E::Win32(error.win32())))?;
    let location = match reader.find_languages(
        &ResourceIdentifier32::Id(24),
        &ResourceIdentifier32::Id(if module.is_dll() { 2 } else { 1 }),
        &[language, language & 0x3ff, 0, 0x409, 9],
        true,
    ) {
        Ok(location) => location,
        Err(
            ResourceError32::TypeNotFound
            | ResourceError32::NameNotFound
            | ResourceError32::LanguageNotFound,
        ) => return Ok(None),
        Err(error) => return Err(HostError::Activation(E::Win32(error.win32()))),
    };
    if location.size > config.manifest_bytes {
        return Err(HostError::Activation(E::NotEnoughMemory));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(location.size as usize)
        .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
    bytes.resize(location.size as usize, 0);
    source
        .read_image(location.data_rva, &mut bytes)
        .map_err(|error| HostError::Activation(E::Win32(error.win32())))?;
    Ok(Some(bytes))
}

impl ModuleActivationWork32 {
    pub(in crate::process) fn leases(&self) -> &[crate::process::windows::files::ProviderFile32] {
        &self.work.leases
    }
}

impl ModuleActivationWork32 {
    pub(in crate::process) fn identity(&self) -> &str {
        &self.identity
    }
    pub(in crate::process) fn graph(
        &self,
    ) -> Option<std::sync::Arc<crate::windows::activation::ActivationGraph32>> {
        self.work.graph.clone()
    }
    pub(in crate::process) fn process_default(&self) -> bool {
        self.process_default
    }
}
