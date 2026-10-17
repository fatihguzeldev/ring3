use crate::{
    files::FileIo32,
    process::{EngineInstance, HostError, windows::files::ProviderFile32},
    windows::user32::User32Error32 as E,
};

pub(super) struct ImageInput32 {
    pub lease: ProviderFile32,
    length: u64,
    bytes: Vec<u8>,
    received: usize,
}

impl ImageInput32 {
    pub(super) fn new(lease: ProviderFile32, length: u64) -> Self {
        Self {
            lease,
            length,
            bytes: Vec::new(),
            received: 0,
        }
    }
    pub(super) fn poll(
        &mut self,
        engine: &mut EngineInstance,
        limit: usize,
    ) -> Result<FileIo32<()>, HostError> {
        let length = usize::try_from(self.length).map_err(|_| HostError::User32(E::Capacity))?;
        if length > limit {
            return Err(HostError::User32(E::Capacity));
        }
        if length < 14 {
            return Err(HostError::User32(E::Win32(13)));
        }
        if self.bytes.len() != length {
            self.bytes
                .try_reserve_exact(length)
                .map_err(|_| HostError::User32(E::Allocation))?;
            self.bytes.resize(length, 0);
        }
        let chunk = engine.provider_file_read_limit()?.min(64 * 1024);
        while self.received < length {
            let count = (length - self.received).min(chunk);
            match engine.read_provider_file(self.lease, self.received as u64, count)? {
                FileIo32::NeedBytes(request) => return Ok(FileIo32::NeedBytes(request)),
                FileIo32::Ready(bytes) => {
                    if bytes.len() != count {
                        return Err(HostError::User32(E::Win32(38)));
                    }
                    self.bytes[self.received..self.received + count].copy_from_slice(&bytes);
                    self.received += count;
                }
            }
        }
        Ok(FileIo32::Ready(()))
    }
    pub(super) fn bytes(&self) -> &[u8] {
        assert_eq!(self.received as u64, self.length);
        &self.bytes
    }
}
