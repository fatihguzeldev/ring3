use crate::{
    files::FileIo32,
    loader::{
        ParserLimits32, PeResourceFile32, PeResources32, ResourceError32, ResourceFileSpan32,
        ResourceIdentifier32,
    },
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
    windows::activation::{ActivationConfig32, ActivationError32 as E},
};

pub(super) struct ManifestInput32 {
    pub(super) path: Vec<u16>,
    pub(super) lease: ProviderFile32,
    pub(super) modified: u64,
    pub(super) resource_name: Option<ResourceIdentifier32>,
    length: u64,
    resource: Option<ResourceIdentifier32>,
    language: u16,
    pe: bool,
    stage: InputStage32,
}
enum InputStage32 {
    Probe,
    Text,
    Header(Vec<u8>),
    Resource {
        metadata: PeResourceFile32,
        spans: Vec<ResourceFileSpan32>,
    },
    Payload {
        offset: u64,
        length: u32,
    },
    Complete,
}
impl ManifestInput32 {
    pub(super) fn new(
        path: Vec<u16>,
        lease: ProviderFile32,
        length: u64,
        modified: u64,
        resource: Option<ResourceIdentifier32>,
        language: u16,
    ) -> Self {
        Self {
            path,
            lease,
            length,
            modified,
            resource_name: None,
            resource,
            language,
            pe: false,
            stage: InputStage32::Probe,
        }
    }
    pub(super) fn implicit_pe_resource(&self) -> bool {
        self.pe && self.resource.is_none()
    }
    pub(super) fn poll(
        &mut self,
        engine: &mut EngineInstance,
        limits: ActivationConfig32,
    ) -> Result<FileIo32<Vec<u8>>, HostError> {
        let metadata_limit = u64::from(limits.manifest_bytes) + u64::from(limits.nodes) * 32;
        loop {
            match &mut self.stage {
                InputStage32::Probe => {
                    match engine.read_provider_file(self.lease, 0, self.length.min(64) as usize)? {
                        FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                        FileIo32::Ready(bytes) => {
                            self.stage = if bytes.starts_with(b"MZ") {
                                self.pe = true;
                                InputStage32::Header(bytes)
                            } else {
                                InputStage32::Text
                            }
                        }
                    }
                }
                InputStage32::Text => {
                    if self.length > u64::from(limits.manifest_bytes) {
                        return Err(HostError::Activation(E::NotEnoughMemory));
                    }
                    match engine.read_provider_file(self.lease, 0, self.length as usize)? {
                        FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                        FileIo32::Ready(bytes) => {
                            self.stage = InputStage32::Complete;
                            return Ok(FileIo32::Ready(bytes));
                        }
                    }
                }
                InputStage32::Header(headers) => {
                    match PeResourceFile32::parse(headers, self.length) {
                        Ok(metadata) => {
                            self.stage = InputStage32::Resource {
                                metadata,
                                spans: Vec::new(),
                            }
                        }
                        Err(ResourceError32::MissingFileRange { offset, length }) => {
                            let required = offset
                                .checked_add(u64::from(length))
                                .filter(|&length| length <= self.length && length <= metadata_limit)
                                .ok_or(HostError::Activation(E::NotEnoughMemory))?;
                            if required <= headers.len() as u64 {
                                return Err(HostError::Activation(E::ManifestFormat));
                            }
                            match engine.read_provider_file(self.lease, 0, required as usize)? {
                                FileIo32::NeedBytes(request) => {
                                    return Ok(FileIo32::NeedBytes(request));
                                }
                                FileIo32::Ready(bytes) => *headers = bytes,
                            }
                        }
                        Err(error) => return Err(HostError::Activation(E::Win32(error.win32()))),
                    }
                }
                InputStage32::Resource { metadata, spans } => {
                    let result = (|| {
                        let mut reader = PeResources32::from_file_parts(
                            metadata,
                            spans,
                            ParserLimits32 {
                                metadata_entries: limits.nodes as usize,
                                string_bytes: limits.manifest_bytes as usize,
                            },
                        )?;
                        let resource = if let Some(resource) = &self.resource {
                            resource.clone()
                        } else {
                            reader
                                .names(&ResourceIdentifier32::Id(24))?
                                .into_iter()
                                .next()
                                .ok_or(ResourceError32::NameNotFound)?
                        };
                        let location = reader.find_languages(
                            &ResourceIdentifier32::Id(24),
                            &resource,
                            &[self.language, self.language & 0x3ff, 0, 0x409, 9],
                            true,
                        )?;
                        reader.file_range(location).map(|range| (range, resource))
                    })();
                    match result {
                        Ok((range, resource)) => {
                            if range.length > limits.manifest_bytes {
                                return Err(HostError::Activation(E::NotEnoughMemory));
                            }
                            self.stage = InputStage32::Payload {
                                offset: range.offset,
                                length: range.length,
                            };
                            self.resource_name = Some(resource);
                        }
                        Err(ResourceError32::MissingFileRange { offset, length }) => {
                            let start = offset & !4095;
                            let end = offset
                                .checked_add(u64::from(length))
                                .and_then(|end| end.checked_add(4095))
                                .map(|end| end & !4095)
                                .filter(|&end| end >= offset)
                                .ok_or(HostError::Activation(E::ManifestFormat))?
                                .min(self.length);
                            let length = (end - start) as usize;
                            let current = spans
                                .iter()
                                .map(|span| span.bytes.len() as u64)
                                .sum::<u64>();
                            if current + length as u64 > metadata_limit {
                                return Err(HostError::Activation(E::NotEnoughMemory));
                            }
                            spans
                                .try_reserve(1)
                                .map_err(|_| HostError::Activation(E::NotEnoughMemory))?;
                            match engine.read_provider_file(self.lease, start, length)? {
                                FileIo32::NeedBytes(request) => {
                                    return Ok(FileIo32::NeedBytes(request));
                                }
                                FileIo32::Ready(bytes) => spans.push(ResourceFileSpan32 {
                                    offset: start,
                                    bytes,
                                }),
                            }
                        }
                        Err(error) => return Err(HostError::Activation(E::Win32(error.win32()))),
                    }
                }
                InputStage32::Payload { offset, length } => {
                    match engine.read_provider_file(self.lease, *offset, *length as usize)? {
                        FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                        FileIo32::Ready(bytes) => {
                            self.stage = InputStage32::Complete;
                            return Ok(FileIo32::Ready(bytes));
                        }
                    }
                }
                InputStage32::Complete => return Err(HostError::Activation(E::Stale)),
            }
        }
    }
}
