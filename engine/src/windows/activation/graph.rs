use super::{ActivationConfig32, ActivationError32 as E, manifest::Manifest32};
use crate::windows::text::ordinal_key;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct ActivationAssembly32 {
    pub manifest: Manifest32,
    pub manifest_path: Vec<u16>,
    pub directory: Vec<u16>,
    pub modified: u64,
    pub resource_name: Option<crate::loader::ResourceIdentifier32>,
}

#[derive(Clone, Debug)]
pub struct ActivationGraph32 {
    assemblies: Vec<ActivationAssembly32>,
    application_directory: Vec<u16>,
    configuration_path: Option<Vec<u16>>,
}
impl ActivationGraph32 {
    pub fn new(
        assemblies: Vec<ActivationAssembly32>,
        limits: ActivationConfig32,
    ) -> Result<Self, E> {
        if assemblies.is_empty() || assemblies.len() > limits.assemblies as usize {
            return Err(E::NotEnoughMemory);
        }
        let mut files = BTreeSet::new();
        let mut classes = BTreeSet::new();
        let mut progids = BTreeSet::new();
        let mut interfaces = BTreeSet::new();
        let mut libraries = BTreeSet::new();
        let mut windows = BTreeSet::new();
        for (index, assembly) in assemblies.iter().enumerate() {
            if assemblies[..index].iter().any(|old| {
                old.manifest.identity.satisfies(&assembly.manifest.identity)
                    && assembly.manifest.identity.satisfies(&old.manifest.identity)
            }) {
                return Err(E::IdentityMismatch);
            }
            for dependency in &assembly.manifest.dependencies {
                let mut identity = dependency.identity.clone();
                for redirect in &dependency.redirects {
                    if (redirect.minimum..=redirect.maximum).contains(&identity.version) {
                        identity.version = redirect.target;
                        break;
                    }
                }
                let resolved = assemblies
                    .iter()
                    .find(|candidate| candidate.manifest.identity.satisfies(&identity));
                if resolved.is_none() && !dependency.optional {
                    return Err(E::AssemblyNotFound);
                }
                if assembly.manifest.no_inherit
                    && resolved.is_some_and(|assembly| !assembly.manifest.no_inheritable)
                {
                    return Err(E::ManifestFormat);
                }
            }
            for file in &assembly.manifest.files {
                if !files.insert(key(&file.name)) {
                    return Err(E::DuplicateDll);
                }
                for class in &file.classes {
                    if !classes.insert(class.clsid) {
                        return Err(E::DuplicateClass);
                    }
                    for progid in &class.progids {
                        if !progids.insert(key(progid)) {
                            return Err(E::DuplicateProgId);
                        }
                    }
                }
                for interface in &file.interfaces {
                    if !interfaces.insert(interface.iid) {
                        return Err(E::DuplicateInterface);
                    }
                }
                for library in &file.type_libraries {
                    if !libraries.insert((library.id, library.version)) {
                        return Err(E::DuplicateTypeLibrary);
                    }
                }
                for window in &file.window_classes {
                    if !windows.insert(key(&window.name)) {
                        return Err(E::DuplicateWindowClass);
                    }
                }
            }
            for interface in &assembly.manifest.interfaces {
                if !interfaces.insert(interface.iid) {
                    return Err(E::DuplicateInterface);
                }
            }
        }
        let application_directory = assemblies[0].directory.clone();
        Ok(Self {
            assemblies,
            application_directory,
            configuration_path: None,
        })
    }
    pub fn assemblies(&self) -> &[ActivationAssembly32] {
        &self.assemblies
    }
    pub fn no_inherit(&self) -> bool {
        self.assemblies[0].manifest.no_inherit
    }
    pub fn with_application_directory(mut self, directory: Vec<u16>) -> Self {
        self.application_directory = directory;
        self
    }
    pub fn with_configuration_path(mut self, path: Option<Vec<u16>>) -> Self {
        self.configuration_path = path;
        self
    }
    pub fn configuration_path(&self) -> Option<&[u16]> {
        self.configuration_path.as_deref()
    }
    pub fn application_directory(&self) -> &[u16] {
        &self.application_directory
    }
}
pub(super) fn key(value: &str) -> Vec<u16> {
    ordinal_key(&value.encode_utf16().collect::<Vec<_>>())
}
pub(super) fn file_path(directory: &[u16], name: &str) -> Vec<u16> {
    let mut path = directory.to_vec();
    if !path.ends_with(&[u16::from(b'\\')]) {
        path.push(u16::from(b'\\'));
    }
    path.extend(name.encode_utf16().map(|unit| {
        if unit == u16::from(b'/') {
            u16::from(b'\\')
        } else {
            unit
        }
    }));
    path
}
