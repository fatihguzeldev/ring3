use crate::{
    files::path::FilePathScope32,
    loader::{ModuleError32, normalize_module_identity, normalize_module_name},
    process::{
        EngineInstance, HostError, windows::activation::modules::ModuleActivationSnapshot32,
    },
};

#[derive(Clone)]
pub(in crate::process) struct CapturedModulePaths32 {
    files: FilePathScope32,
    application: Vec<u16>,
    system: Vec<u16>,
    directories: Vec<Vec<u16>>,
    activation: ModuleActivationSnapshot32,
    importers: Vec<(String, ModuleActivationSnapshot32)>,
}

impl CapturedModulePaths32 {
    pub(in crate::process) fn has_importer_activation(&self, identity: &str) -> bool {
        self.importers.iter().any(|(name, _)| name == identity)
    }

    pub(in crate::process) fn set_process_default_activation(
        &mut self,
        graph: ModuleActivationSnapshot32,
    ) {
        self.activation = graph;
    }
    pub(in crate::process) fn set_importer_activation(
        &mut self,
        identity: String,
        graph: ModuleActivationSnapshot32,
    ) -> Result<(), HostError> {
        if let Some((_, current)) = self
            .importers
            .iter_mut()
            .find(|(name, _)| *name == identity)
        {
            *current = graph;
        } else {
            if self.importers.len() >= 4096 {
                return Err(HostError::Module(ModuleError32::Capacity));
            }
            self.importers
                .try_reserve(1)
                .map_err(|_| HostError::Infrastructure)?;
            self.importers.push((identity, graph));
        }
        Ok(())
    }
}

impl EngineInstance {
    pub(in crate::process) fn capture_module_paths(
        &self,
        initial_root: Option<&str>,
    ) -> Result<CapturedModulePaths32, HostError> {
        let files = self.capture_module_file_paths();
        let application =
            if let Some(root) = initial_root.filter(|path| path.contains(['/', '\\', ':'])) {
                let path = files
                    .normalize(&root.encode_utf16().collect::<Vec<_>>())
                    .map_err(HostError::File)?;
                crate::files::path::parent(&path)
                    .map_err(HostError::File)?
                    .to_vec()
            } else {
                self.module_application_directory()
            };
        let application = files
            .normalize(if application.is_empty() {
                &[46]
            } else {
                &application
            })
            .map_err(HostError::File)?;
        let mut directories = self.module_file_search_directories();
        let system = directories.first().cloned().unwrap_or_default();
        directories.insert(0, application.clone());
        if let Some(path) = self.process_path_environment() {
            for (index, directory) in path.split(';').filter(|part| !part.is_empty()).enumerate() {
                if index >= 256 {
                    return Err(HostError::Module(ModuleError32::Capacity));
                }
                directories.push(directory.encode_utf16().collect());
            }
        }
        let directories = directories
            .iter()
            .map(|directory| files.normalize(directory).map_err(HostError::File))
            .collect::<Result<Vec<_>, _>>()?;
        let system = files.normalize(&system).map_err(HostError::File)?;
        Ok(CapturedModulePaths32 {
            files,
            application,
            system,
            directories,
            activation: self.capture_module_activation()?,
            importers: self.capture_loaded_module_activations()?,
        })
    }

    pub(in crate::process) fn resolve_module_source_captured(
        &self,
        request: &str,
        paths: &CapturedModulePaths32,
    ) -> Result<String, HostError> {
        let redirect = paths
            .activation
            .dll_redirection(&request.encode_utf16().collect::<Vec<_>>());
        self.resolve_module_source_redirected(request, paths, redirect)
    }

    pub(in crate::process) fn resolve_module_import_captured(
        &self,
        importer: &str,
        request: &str,
        paths: &CapturedModulePaths32,
    ) -> Result<String, HostError> {
        let name = request.encode_utf16().collect::<Vec<_>>();
        let redirect = paths
            .importers
            .iter()
            .find(|(identity, _)| identity == importer)
            .and_then(|(_, graph)| graph.dll_redirection(&name))
            .or_else(|| paths.activation.dll_redirection(&name));
        self.resolve_module_source_redirected(request, paths, redirect)
    }

    fn resolve_module_source_redirected(
        &self,
        request: &str,
        paths: &CapturedModulePaths32,
        redirect: Option<Vec<u16>>,
    ) -> Result<String, HostError> {
        let request = redirect
            .map(|path| {
                String::from_utf16(&path).map_err(|_| HostError::Module(ModuleError32::InvalidName))
            })
            .transpose()?
            .unwrap_or_else(|| request.to_owned());
        let catalog = &self.modules.catalog;
        let qualified = request.contains(['/', '\\', ':']);
        if !qualified {
            let name = normalize_module_name(&request).map_err(HostError::Module)?;
            if let Some(module) = self.modules.loaded.find(&name) {
                return Ok(module.identity().to_owned());
            }
            if catalog.is_system(&name) {
                return Ok(name);
            }
        }
        if qualified {
            let path = paths
                .files
                .normalize(&request.encode_utf16().collect::<Vec<_>>())
                .map_err(HostError::File)?;
            return self.resolve_exact_module_path(&path, paths);
        }
        let name = normalize_module_name(&request).map_err(HostError::Module)?;
        let mut first = None;
        for directory in &paths.directories {
            let mut path = directory.clone();
            if path.last() != Some(&92) {
                path.push(92);
            }
            path.extend(name.encode_utf16());
            let path = paths.files.normalize(&path).map_err(HostError::File)?;
            let identity = self.resolve_exact_module_path(&path, paths)?;
            if catalog.contains(&identity) || self.module_source_file_exists(&path) {
                return Ok(identity);
            }
            first.get_or_insert(identity);
        }
        Ok(first.unwrap_or(name))
    }

    fn resolve_exact_module_path(
        &self,
        path: &[u16],
        paths: &CapturedModulePaths32,
    ) -> Result<String, HostError> {
        let identity = normalize_module_identity(
            &String::from_utf16(path).map_err(|_| HostError::Module(ModuleError32::InvalidName))?,
        )
        .map_err(HostError::Module)?;
        if self.modules.catalog.contains(&identity) {
            return Ok(identity);
        }
        if let Ok(name) = String::from_utf16(crate::files::path::name(path))
            && let Ok(name) = normalize_module_name(&name)
        {
            let mut implied = if self.modules.catalog.is_system(&name) {
                paths.system.clone()
            } else {
                paths.application.clone()
            };
            if implied.last() != Some(&92) {
                implied.push(92);
            }
            implied.extend(name.encode_utf16());
            let implied = paths.files.normalize(&implied).map_err(HostError::File)?;
            let implied = normalize_module_identity(
                &String::from_utf16(&implied)
                    .map_err(|_| HostError::Module(ModuleError32::InvalidName))?,
            )
            .map_err(HostError::Module)?;
            if implied == identity && self.modules.catalog.contains(&name) {
                return Ok(name);
            }
        }
        Ok(identity)
    }
}
