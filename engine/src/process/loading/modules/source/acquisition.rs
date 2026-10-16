use crate::{
    files::FileIo32,
    loader::ModuleError32,
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
};
pub(super) struct ModuleFileInput32 {
    pub(super) lease: ProviderFile32,
    pub(super) name: String,
    length: usize,
    bytes: Vec<u8>,
}
impl ModuleFileInput32 {
    pub(super) fn new(engine: &mut EngineInstance, name: String) -> Result<Self, HostError> {
        if engine.files.is_none() {
            return Err(HostError::Module(ModuleError32::MissingModule));
        }
        let lease = engine.open_provider_file(&name.encode_utf16().collect::<Vec<_>>(), false)?;
        Ok(Self {
            lease,
            name,
            length: 0,
            bytes: Vec::new(),
        })
    }
    pub(super) fn poll(&mut self, engine: &mut EngineInstance) -> Result<FileIo32<()>, HostError> {
        if self.length == 0 {
            let length = engine.provider_file_size(self.lease)?;
            if length == 0
                || length > engine.modules.catalog.remaining_source_bytes() as u64
                || length > u64::from(u32::MAX)
            {
                return Err(HostError::Module(ModuleError32::Capacity));
            }
            self.bytes
                .try_reserve_exact(length as usize)
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            self.length = length as usize;
        }
        while self.bytes.len() < self.length {
            let length = (self.length - self.bytes.len()).min(65536);
            match engine.read_provider_file(self.lease, self.bytes.len() as u64, length)? {
                FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                FileIo32::Ready(bytes) => {
                    if bytes.len() != length {
                        return Err(HostError::Module(ModuleError32::InvalidRequest));
                    }
                    self.bytes.extend_from_slice(&bytes);
                }
            }
        }
        let version = engine.provider_file_version(self.lease)?;
        engine
            .modules
            .vfs_sources
            .try_reserve(1)
            .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
        engine
            .modules
            .catalog
            .add_guest(&self.name, &self.bytes)
            .map_err(HostError::Module)?;
        engine
            .modules
            .vfs_sources
            .push(super::versions::ModuleVfsSource32 {
                identity: self.name.clone(),
                version,
            });
        Ok(FileIo32::Ready(()))
    }
}
