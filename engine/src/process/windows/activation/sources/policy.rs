use super::search::ManifestCandidate32;
use crate::{
    files::{FileError32, FileIo32},
    loader::ResourceIdentifier32,
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
    windows::activation::{ActivationConfig32, ActivationError32 as E, policy::BindingPolicy32},
};

pub(super) struct PolicyInput32 {
    pub(super) path: Vec<u16>,
    current: Option<(ProviderFile32, u64)>,
    absent: bool,
}
impl PolicyInput32 {
    pub(super) fn new(path: Vec<u16>) -> Self {
        Self {
            path,
            current: None,
            absent: false,
        }
    }
    pub(super) fn poll(
        &mut self,
        engine: &mut EngineInstance,
        leases: &mut Vec<ProviderFile32>,
        limits: ActivationConfig32,
    ) -> Result<FileIo32<Option<Vec<u8>>>, HostError> {
        if self.absent || engine.files.is_none() {
            return Ok(FileIo32::Ready(None));
        }
        if self.current.is_none() {
            leases
                .try_reserve(1)
                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
            match engine.open_provider_file(&self.path, false) {
                Ok(lease) => {
                    leases.push(lease);
                    self.path = engine.provider_file_path(lease)?;
                    let length = engine.provider_file_size(lease)?;
                    if length > u64::from(limits.manifest_bytes) {
                        return Err(HostError::Activation(E::NotEnoughMemory));
                    }
                    self.current = Some((lease, length));
                }
                Err(HostError::File(FileError32::NotFound | FileError32::PathNotFound)) => {
                    self.absent = true;
                    return Ok(FileIo32::Ready(None));
                }
                Err(error) => return Err(error),
            }
        }
        let (lease, length) = self.current.unwrap();
        engine
            .read_provider_file(lease, 0, length as usize)
            .map(|result| match result {
                FileIo32::Ready(bytes) => FileIo32::Ready(Some(bytes)),
                FileIo32::NeedBytes(request) => FileIo32::NeedBytes(request),
            })
    }
}
pub(super) fn configuration_name(
    path: &[u16],
    resource: Option<&ResourceIdentifier32>,
) -> Vec<u16> {
    let mut path = path.to_vec();
    if resource.is_none() {
        let suffix = ".manifest".encode_utf16().collect::<Vec<_>>();
        if path.len() >= suffix.len()
            && path[path.len() - suffix.len()..]
                .iter()
                .zip(&suffix)
                .all(|(&a, &b)| a == b || a == b - 32)
        {
            path.truncate(path.len() - suffix.len());
        }
    } else if let Some(ResourceIdentifier32::Id(id)) = resource
        && *id != 1
    {
        path.extend(format!(".{id}").encode_utf16());
    }
    path.extend(".config".encode_utf16());
    path
}

pub(super) struct PublisherInput32 {
    candidates: Vec<ManifestCandidate32>,
    next: usize,
    current: Option<PolicyInput32>,
    best: Option<([u16; 4], BindingPolicy32)>,
}
impl PublisherInput32 {
    pub(super) fn new(candidates: Vec<ManifestCandidate32>) -> Self {
        Self {
            candidates,
            next: 0,
            current: None,
            best: None,
        }
    }
    pub(super) fn poll(
        &mut self,
        engine: &mut EngineInstance,
        leases: &mut Vec<ProviderFile32>,
        limits: ActivationConfig32,
        source_cost: &mut usize,
    ) -> Result<FileIo32<Option<BindingPolicy32>>, HostError> {
        loop {
            if let Some(input) = &mut self.current {
                match input.poll(engine, leases, limits)? {
                    FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                    FileIo32::Ready(Some(bytes)) => {
                        *source_cost = source_cost
                            .checked_add(bytes.len())
                            .filter(|&cost| cost <= limits.manifest_bytes as usize)
                            .ok_or(HostError::Activation(E::NotEnoughMemory))?;
                        let policy = BindingPolicy32::publisher(&bytes, limits)
                            .map_err(HostError::Activation)?;
                        let version = policy
                            .identity
                            .as_ref()
                            .ok_or(HostError::Activation(E::ManifestFormat))?
                            .version;
                        if self.best.as_ref().is_none_or(|(old, _)| version > *old) {
                            self.best = Some((version, policy));
                        }
                    }
                    FileIo32::Ready(None) => {}
                }
                self.current = None;
            }
            let Some(candidate) = self.candidates.get(self.next) else {
                return Ok(FileIo32::Ready(self.best.take().map(|(_, policy)| policy)));
            };
            self.next += 1;
            self.current = Some(PolicyInput32::new(candidate.path.clone()));
        }
    }
}
