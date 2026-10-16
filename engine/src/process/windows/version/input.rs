use crate::{
    files::FileIo32,
    loader::{
        ParserLimits32, PeResourceFile32, PeResources32, ResourceError32, ResourceFileSpan32,
        ResourceIdentifier32,
    },
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
    windows::version::VersionError32 as E,
};

pub(super) struct VersionInput32 {
    pub(super) lease: ProviderFile32,
    length: u64,
    language: u16,
    stage: Stage32,
}
enum Stage32 {
    Header(Vec<u8>),
    Directory {
        metadata: PeResourceFile32,
        spans: Vec<ResourceFileSpan32>,
    },
    Payload {
        offset: u64,
        length: u32,
    },
    Complete,
}
impl VersionInput32 {
    pub(super) fn new(lease: ProviderFile32, length: u64, language: u16) -> Self {
        Self {
            lease,
            length,
            language,
            stage: Stage32::Header(Vec::new()),
        }
    }
    pub(super) fn poll(
        &mut self,
        engine: &mut EngineInstance,
    ) -> Result<FileIo32<Vec<u8>>, HostError> {
        loop {
            match &mut self.stage {
                Stage32::Header(bytes) => match PeResourceFile32::parse(bytes, self.length) {
                    Ok(metadata) => {
                        self.stage = Stage32::Directory {
                            metadata,
                            spans: Vec::new(),
                        }
                    }
                    Err(ResourceError32::MissingFileRange { offset, length }) => {
                        let required = offset
                            .checked_add(u64::from(length))
                            .filter(|&n| n <= self.length && n <= 1024 * 1024)
                            .ok_or(HostError::Version(E::InvalidData))?;
                        if required <= bytes.len() as u64 {
                            return Err(HostError::Version(E::InvalidData));
                        }
                        match engine.read_provider_file(self.lease, 0, required as usize)? {
                            FileIo32::NeedBytes(r) => return Ok(FileIo32::NeedBytes(r)),
                            FileIo32::Ready(data) => *bytes = data,
                        }
                    }
                    Err(e) => return Err(HostError::Version(E::Win32(e.win32()))),
                },
                Stage32::Directory { metadata, spans } => {
                    let found = (|| {
                        let mut reader = PeResources32::from_file_parts(
                            metadata,
                            spans,
                            ParserLimits32 {
                                metadata_entries: 4096,
                                string_bytes: 131072,
                            },
                        )?;
                        let location = reader.find_languages(
                            &ResourceIdentifier32::Id(16),
                            &ResourceIdentifier32::Id(1),
                            &[self.language, self.language & 0x3ff, 0, 0x409, 9],
                            true,
                        )?;
                        reader.file_range(location)
                    })();
                    match found {
                        Ok(range) => {
                            if range.length > 65535 {
                                return Err(HostError::Version(E::InvalidData));
                            }
                            self.stage = Stage32::Payload {
                                offset: range.offset,
                                length: range.length,
                            };
                        }
                        Err(ResourceError32::MissingFileRange { offset, length }) => {
                            let start = offset & !4095;
                            let end = offset
                                .checked_add(u64::from(length))
                                .and_then(|n| n.checked_add(4095))
                                .map(|n| n & !4095)
                                .ok_or(HostError::Version(E::InvalidData))?
                                .min(self.length);
                            let length = (end - start) as usize;
                            if spans.iter().map(|s| s.bytes.len()).sum::<usize>() + length
                                > 1024 * 1024
                            {
                                return Err(HostError::Version(E::Allocation));
                            }
                            spans
                                .try_reserve(1)
                                .map_err(|_| HostError::Version(E::Allocation))?;
                            match engine.read_provider_file(self.lease, start, length)? {
                                FileIo32::NeedBytes(r) => return Ok(FileIo32::NeedBytes(r)),
                                FileIo32::Ready(bytes) => spans.push(ResourceFileSpan32 {
                                    offset: start,
                                    bytes,
                                }),
                            }
                        }
                        Err(e) => return Err(HostError::Version(E::Win32(e.win32()))),
                    }
                }
                Stage32::Payload { offset, length } => {
                    match engine.read_provider_file(self.lease, *offset, *length as usize)? {
                        FileIo32::NeedBytes(r) => return Ok(FileIo32::NeedBytes(r)),
                        FileIo32::Ready(bytes) => {
                            self.stage = Stage32::Complete;
                            return Ok(FileIo32::Ready(bytes));
                        }
                    }
                }
                Stage32::Complete => return Err(HostError::Version(E::InvalidParameter)),
            }
        }
    }
}
