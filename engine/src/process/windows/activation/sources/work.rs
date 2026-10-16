use super::{
    CreateSource32,
    input::ManifestInput32,
    policy::{PolicyInput32, PublisherInput32, configuration_name},
    search::{ManifestCandidate32, absolute, candidates, directory, join, publisher_candidates},
};
use crate::{
    files::{FileError32, FileIo32},
    loader::ResourceIdentifier32,
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
    windows::activation::{
        ActivationAssembly32, ActivationConfig32, ActivationError32 as E, ActivationGraph32,
        manifest::{AssemblyIdentity32, Manifest32},
        policy::BindingPolicy32,
    },
};
use std::sync::Arc;

pub(in crate::process::windows::activation) struct CreateWork32 {
    pub(in crate::process::windows::activation) specification: CreateSource32,
    pub(in crate::process::windows::activation) leases: Vec<ProviderFile32>,
    pub(in crate::process::windows::activation) graph: Option<Arc<ActivationGraph32>>,
    pub(in crate::process::windows::activation) error: Option<HostError>,
    pub(in crate::process::windows::activation) source_cost: usize,
    assemblies: Vec<ActivationAssembly32>,
    tasks: Vec<ManifestTask32>,
    current: Option<(ManifestTask32, ManifestInput32)>,
    directory: Vec<u16>,
    application_directory: Vec<u16>,
    initialized: bool,
    processed: usize,
    configuration: Option<PolicyInput32>,
    configuration_path: Option<Vec<u16>>,
    policy: BindingPolicy32,
    policy_loaded: bool,
}
struct ManifestTask32 {
    candidates: Vec<ManifestCandidate32>,
    directory: Option<Vec<u16>>,
    language: Option<String>,
    next: usize,
    identity: Option<AssemblyIdentity32>,
    optional: bool,
    resource: Option<ResourceIdentifier32>,
    publisher: Option<PublisherInput32>,
    reference: Option<(usize, usize)>,
    associated_error: Option<HostError>,
}
struct ManifestProvenance32 {
    modified: u64,
    resource: Option<ResourceIdentifier32>,
    directory: Option<Vec<u16>>,
    language: Option<String>,
}
impl CreateWork32 {
    pub(in crate::process::windows::activation) fn new(specification: CreateSource32) -> Self {
        Self {
            specification,
            leases: Vec::new(),
            graph: None,
            error: None,
            source_cost: 0,
            assemblies: Vec::new(),
            tasks: Vec::new(),
            current: None,
            directory: Vec::new(),
            application_directory: Vec::new(),
            initialized: false,
            processed: 0,
            configuration: None,
            configuration_path: None,
            policy: BindingPolicy32::default(),
            policy_loaded: false,
        }
    }
    pub(in crate::process::windows::activation) fn poll(
        &mut self,
        engine: &mut EngineInstance,
        limits: ActivationConfig32,
    ) -> Result<FileIo32<()>, HostError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.graph.is_some() {
            return Ok(FileIo32::Ready(()));
        }
        let result = self.poll_inner(engine, limits);
        if let Err(error) = result {
            self.error = Some(error);
        }
        result
    }
    fn poll_inner(
        &mut self,
        engine: &mut EngineInstance,
        limits: ActivationConfig32,
    ) -> Result<FileIo32<()>, HostError> {
        if !self.initialized {
            if let Some(module) = self.specification.module {
                let path = engine.resource_module_file_name(module)?;
                self.application_directory = self
                    .specification
                    .application
                    .clone()
                    .unwrap_or_else(|| directory(&path));
                let bytes = engine.resource_data_bytes(
                    module,
                    &ResourceIdentifier32::Id(24),
                    self.specification.resource.as_ref().unwrap(),
                    self.specification.language,
                    limits.manifest_bytes as usize,
                )?;
                self.directory = self
                    .specification
                    .assembly_directory
                    .clone()
                    .unwrap_or_else(|| directory(&path));
                let provenance = ManifestProvenance32 {
                    modified: 0,
                    resource: self.specification.resource.clone(),
                    directory: None,
                    language: None,
                };
                self.accept(&bytes, path, None, provenance, limits)?;
            } else {
                self.application_directory =
                    if let Some(application) = &self.specification.application {
                        application.clone()
                    } else {
                        directory(&engine.virtual_module_file_name_units(0)?)
                    };
                let mut path = self
                    .specification
                    .source
                    .clone()
                    .ok_or(HostError::Activation(E::InvalidParameter))?;
                if let Some(directory) = &self.specification.assembly_directory
                    && !absolute(&path)
                {
                    path = join(directory, &path);
                }
                self.tasks.push(ManifestTask32 {
                    candidates: vec![ManifestCandidate32 {
                        path,
                        directory: None,
                        language: None,
                    }],
                    directory: None,
                    language: None,
                    next: 0,
                    identity: None,
                    optional: false,
                    resource: self.specification.resource.clone(),
                    publisher: None,
                    reference: None,
                    associated_error: None,
                });
            }
            self.initialized = true;
        }
        'poll: loop {
            if self.processed < self.assemblies.len() {
                if !self.policy_loaded {
                    let root = &self.assemblies[0];
                    let input = self.configuration.get_or_insert_with(|| {
                        PolicyInput32::new(configuration_name(
                            &root.manifest_path,
                            root.resource_name.as_ref(),
                        ))
                    });
                    match input.poll(engine, &mut self.leases, limits)? {
                        FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                        FileIo32::Ready(Some(bytes)) => {
                            self.source_cost = self
                                .source_cost
                                .checked_add(bytes.len())
                                .filter(|&cost| cost <= limits.manifest_bytes as usize)
                                .ok_or(HostError::Activation(E::NotEnoughMemory))?;
                            self.policy = BindingPolicy32::application(&bytes, limits)
                                .map_err(HostError::Activation)?;
                            if let Some(reference) = &self.policy.identity {
                                let mut reference = reference.clone();
                                if reference.version == [0; 4] {
                                    reference.version = root.manifest.identity.version;
                                }
                                if !root.manifest.identity.satisfies(&reference) {
                                    return Err(HostError::Activation(E::IdentityMismatch));
                                }
                            }
                            self.configuration_path = Some(input.path.clone());
                        }
                        FileIo32::Ready(None) => {}
                    }
                    self.configuration = None;
                    self.policy_loaded = true;
                }
                self.enqueue_dependencies(engine, self.processed, limits)?;
                self.processed += 1;
                continue;
            }
            if let Some((task, input)) = &mut self.current {
                match input.poll(engine, limits) {
                    Ok(FileIo32::NeedBytes(request)) => return Ok(FileIo32::NeedBytes(request)),
                    Err(error @ HostError::Activation(E::Win32(1813..=1815)))
                        if task.identity.is_none()
                            && task.associated_error.is_none()
                            && input.implicit_pe_resource() =>
                    {
                        let mut path = input.path.clone();
                        path.extend(".manifest".encode_utf16());
                        self.tasks
                            .try_reserve(1)
                            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
                        let (mut task, _) = self.current.take().unwrap();
                        task.candidates.clear();
                        task.candidates.push(ManifestCandidate32 {
                            path,
                            directory: None,
                            language: None,
                        });
                        task.next = 0;
                        task.associated_error = Some(error);
                        self.tasks.push(task);
                        continue;
                    }
                    Err(error) => return Err(error),
                    Ok(FileIo32::Ready(bytes)) => {
                        let path = input.path.clone();
                        let mut expected = task.identity.clone();
                        if let Some(identity) = &mut expected
                            && let Some(language) = &task.language
                        {
                            identity.language = if language.is_empty() {
                                "*".into()
                            } else {
                                language.clone()
                            };
                        }
                        let provenance = ManifestProvenance32 {
                            modified: input.modified,
                            resource: input.resource_name.clone(),
                            directory: task.directory.clone(),
                            language: task.language.clone(),
                        };
                        let reference = task.reference;
                        if self.assemblies.is_empty() {
                            self.directory = self
                                .specification
                                .assembly_directory
                                .clone()
                                .unwrap_or_else(|| directory(&path));
                        }
                        self.accept(&bytes, path, expected.as_ref(), provenance, limits)?;
                        if let Some((assembly, dependency)) = reference {
                            self.assemblies[assembly].manifest.dependencies[dependency].identity =
                                expected.unwrap();
                        }
                        self.current = None;
                        continue;
                    }
                }
            }
            let Some(mut task) = self.tasks.pop() else {
                let assemblies = core::mem::take(&mut self.assemblies);
                self.graph = Some(Arc::new(
                    ActivationGraph32::new(assemblies, limits)
                        .map_err(HostError::Activation)?
                        .with_application_directory(self.application_directory.clone())
                        .with_configuration_path(self.configuration_path.clone()),
                ));
                return Ok(FileIo32::Ready(()));
            };
            if let Some(publisher) = &mut task.publisher {
                let policy = match publisher.poll(
                    engine,
                    &mut self.leases,
                    limits,
                    &mut self.source_cost,
                )? {
                    FileIo32::NeedBytes(request) => {
                        self.tasks.push(task);
                        return Ok(FileIo32::NeedBytes(request));
                    }
                    FileIo32::Ready(policy) => policy,
                };
                let identity = task.identity.as_mut().unwrap();
                if let Some(policy) = policy {
                    let header = policy
                        .identity
                        .as_ref()
                        .ok_or(HostError::Activation(E::ManifestFormat))?;
                    if !header.name.eq_ignore_ascii_case(&format!(
                        "policy.{}.{}.{}",
                        identity.version[0], identity.version[1], identity.name
                    )) || header.public_key_token != identity.public_key_token
                    {
                        return Err(HostError::Activation(E::IdentityMismatch));
                    }
                    if let Some(version) =
                        policy.redirected(identity).map_err(HostError::Activation)?
                    {
                        identity.version = version;
                    }
                }
                if let Some((assembly, dependency)) = task.reference {
                    self.assemblies[assembly].manifest.dependencies[dependency].identity =
                        identity.clone();
                }
                task.candidates = self.dependency_candidates(engine, identity, limits)?;
                task.publisher = None;
            }
            if task.identity.as_ref().is_some_and(|identity| {
                self.assemblies
                    .iter()
                    .any(|assembly| assembly.manifest.identity.satisfies(identity))
            }) {
                continue;
            }
            let optional = task.optional;
            let root_source = task.identity.is_none();
            let mut missing = None;
            while let Some(candidate) = task.candidates.get(task.next) {
                task.next += 1;
                self.leases
                    .try_reserve(1)
                    .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
                match engine.open_provider_file(&candidate.path, false) {
                    Ok(lease) => {
                        task.directory = candidate.directory.clone();
                        task.language = candidate.language.clone();
                        self.leases.push(lease);
                        let path = engine.provider_file_path(lease)?;
                        let info = engine.provider_file_info(lease)?;
                        let input = ManifestInput32::new(
                            path,
                            lease,
                            info.size,
                            info.times.written,
                            task.resource.clone(),
                            self.specification.language,
                        );
                        self.current = Some((task, input));
                        continue 'poll;
                    }
                    Err(
                        error @ HostError::File(FileError32::NotFound | FileError32::PathNotFound),
                    ) => {
                        missing = Some(error);
                    }
                    Err(error) => return Err(error),
                }
            }
            if !optional {
                if root_source {
                    return Err(task
                        .associated_error
                        .or(missing)
                        .unwrap_or(HostError::Activation(E::FileNotFound)));
                }
                return Err(HostError::Activation(E::AssemblyNotFound));
            }
        }
    }
    fn accept(
        &mut self,
        bytes: &[u8],
        path: Vec<u16>,
        expected: Option<&AssemblyIdentity32>,
        provenance: ManifestProvenance32,
        limits: ActivationConfig32,
    ) -> Result<(), HostError> {
        self.source_cost = self
            .source_cost
            .checked_add(bytes.len())
            .filter(|&length| length <= limits.manifest_bytes as usize)
            .ok_or(HostError::Activation(E::NotEnoughMemory))?;
        let manifest = Manifest32::parse(bytes, limits).map_err(HostError::Activation)?;
        if expected.is_some_and(|expected| !manifest.identity.satisfies(expected))
            || provenance.language.as_ref().is_some_and(|language| {
                !manifest
                    .identity
                    .language
                    .eq_ignore_ascii_case(if language.is_empty() { "*" } else { language })
            })
        {
            return Err(HostError::Activation(E::IdentityMismatch));
        }
        if self.assemblies.len() >= limits.assemblies as usize {
            return Err(HostError::Activation(E::NotEnoughMemory));
        }
        self.assemblies
            .try_reserve(1)
            .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
        let assembly_directory = if self.assemblies.is_empty() {
            self.directory.clone()
        } else {
            provenance.directory.unwrap_or_else(|| directory(&path))
        };
        self.assemblies.push(ActivationAssembly32 {
            manifest,
            manifest_path: path,
            directory: assembly_directory,
            modified: provenance.modified,
            resource_name: provenance.resource,
        });
        Ok(())
    }
    fn enqueue_dependencies(
        &mut self,
        engine: &EngineInstance,
        index: usize,
        limits: ActivationConfig32,
    ) -> Result<(), HostError> {
        let dependencies = self.assemblies[index].manifest.dependencies.clone();
        for (dependency_index, dependency) in dependencies.iter().enumerate().rev() {
            let mut identity = dependency.identity.clone();
            for redirect in &dependency.redirects {
                if (redirect.minimum..=redirect.maximum).contains(&identity.version) {
                    identity.version = redirect.target;
                    break;
                }
            }
            let application_version = self
                .policy
                .redirected(&identity)
                .map_err(HostError::Activation)?;
            let app_redirect = application_version.is_some();
            if let Some(version) = application_version {
                identity.version = version;
            }
            self.assemblies[index].manifest.dependencies[dependency_index].identity =
                identity.clone();
            self.assemblies[index].manifest.dependencies[dependency_index]
                .redirects
                .clear();
            if self
                .assemblies
                .iter()
                .any(|assembly| assembly.manifest.identity.satisfies(&identity))
                || self
                    .tasks
                    .iter()
                    .any(|task| task.identity.as_ref() == Some(&identity))
            {
                continue;
            }
            if self.tasks.len() + self.assemblies.len() >= limits.assemblies as usize {
                return Err(HostError::Activation(E::NotEnoughMemory));
            }
            self.tasks
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
            let publisher =
                if self.policy.publisher && !app_redirect && identity.public_key_token.is_some() {
                    Some(PublisherInput32::new(publisher_candidates(
                        engine,
                        &identity,
                        limits.assemblies as usize,
                    )?))
                } else {
                    None
                };
            self.tasks.push(ManifestTask32 {
                candidates: self.dependency_candidates(engine, &identity, limits)?,
                next: 0,
                directory: None,
                language: None,
                identity: Some(identity),
                optional: dependency.optional,
                resource: Some(ResourceIdentifier32::Id(1)),
                publisher,
                reference: Some((index, dependency_index)),
                associated_error: None,
            });
        }
        Ok(())
    }
    fn dependency_candidates(
        &self,
        engine: &EngineInstance,
        identity: &AssemblyIdentity32,
        limits: ActivationConfig32,
    ) -> Result<Vec<ManifestCandidate32>, HostError> {
        let mut paths = candidates(
            engine,
            &self.application_directory,
            identity,
            self.specification.language,
            limits.assemblies as usize,
        )?;
        for path in &self.policy.private_paths {
            paths.extend(candidates(
                engine,
                &join(&self.application_directory, path),
                identity,
                self.specification.language,
                limits.assemblies as usize,
            )?);
        }
        let pending = self
            .tasks
            .iter()
            .flat_map(|task| &task.candidates)
            .chain(paths.iter())
            .map(|candidate| {
                (candidate.path.len() + candidate.directory.as_ref().map_or(0, Vec::len)) * 2
            })
            .sum::<usize>();
        if pending > limits.section_bytes as usize {
            return Err(HostError::Activation(E::NotEnoughMemory));
        }
        Ok(paths)
    }
}
