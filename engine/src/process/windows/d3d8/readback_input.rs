use super::{D3D8Error32, D3D8ReadbackOwner32};
use crate::{
    abi::graphics::d3d8::{READBACK_HEADER_SIZE, ReadbackHeader, ReadbackStatus, readback_header},
    process::{CallError, EngineInstance, HostError},
};

pub(super) struct ReadbackInput32 {
    header: ReadbackHeader,
    bytes: Vec<u8>,
    owner: D3D8ReadbackOwner32,
    generation: u64,
}

impl ReadbackInput32 {
    pub(super) fn epoch(&self) -> u64 {
        self.header.id.epoch
    }
}

impl EngineInstance {
    pub fn begin_d3d8_readback_input(&mut self, header: &[u8]) -> Result<(), HostError> {
        self.memory()?;
        let decoded = readback_header(header).map_err(|_| HostError::InvalidRequest)?;
        let runtime = self
            .d3d8
            .as_ref()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        if runtime.readback_input.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        let owner = self.d3d8_readback_owner(decoded.id)?;
        let device = runtime
            .device_by_epoch(decoded.id.epoch)
            .map_err(HostError::D3D8)?;
        let (generation, _, source, destination) =
            device.pending_readback_details(decoded.id).ok_or(stale())?;
        if source != decoded.source || generation != device.generation() {
            return Err(stale());
        }
        let expected = if decoded.status == ReadbackStatus::Complete {
            device
                .resources()
                .info(destination.surface)
                .map_err(|error| HostError::D3D8(D3D8Error32::Device(error.into())))?
                .length
        } else {
            0
        };
        if decoded.payload_bytes != expected {
            return Err(HostError::InvalidRequest);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(READBACK_HEADER_SIZE + expected as usize)
            .map_err(|_| HostError::Infrastructure)?;
        bytes.extend_from_slice(header);
        self.d3d8.as_mut().unwrap().readback_input = Some(ReadbackInput32 {
            header: decoded,
            bytes,
            owner,
            generation,
        });
        Ok(())
    }

    pub fn append_d3d8_readback_input(
        &mut self,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), HostError> {
        self.check_d3d8_readback_input()?;
        let staged = self.d3d8.as_mut().unwrap().readback_input.as_mut().unwrap();
        if bytes.is_empty()
            || offset as usize != staged.bytes.len() - READBACK_HEADER_SIZE
            || bytes.len() > staged.header.payload_bytes as usize - offset as usize
        {
            return Err(HostError::InvalidRequest);
        }
        staged.bytes.extend_from_slice(bytes);
        Ok(())
    }

    pub fn finish_d3d8_readback_input(&mut self) -> Result<(), HostError> {
        self.check_d3d8_readback_input()?;
        let staged = self.d3d8.as_ref().unwrap().readback_input.as_ref().unwrap();
        if staged.bytes.len() != READBACK_HEADER_SIZE + staged.header.payload_bytes as usize {
            return Err(HostError::InvalidRequest);
        }
        let staged = self.d3d8.as_mut().unwrap().readback_input.take().unwrap();
        let result = self.d3d8_complete_owned_readback(&staged.bytes, staged.owner);
        if result.is_err() {
            self.d3d8.as_mut().unwrap().readback_input = Some(staged);
        }
        result
    }

    pub fn abort_d3d8_readback_input(&mut self) -> Result<(), HostError> {
        self.memory()?;
        self.d3d8
            .as_mut()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?
            .readback_input = None;
        Ok(())
    }

    fn check_d3d8_readback_input(&self) -> Result<(), HostError> {
        self.memory()?;
        let runtime = self
            .d3d8
            .as_ref()
            .ok_or(HostError::D3D8(D3D8Error32::NotConfigured))?;
        let staged = runtime.readback_input.as_ref().ok_or(stale())?;
        self.checked_d3d8_readback_owner(staged.owner, staged.header.id)?;
        let device = runtime
            .device_by_epoch(staged.header.id.epoch)
            .map_err(HostError::D3D8)?;
        let (generation, _, source, _) = device
            .pending_readback_details(staged.header.id)
            .ok_or(stale())?;
        if generation != staged.generation
            || generation != device.generation()
            || source != staged.header.source
        {
            return Err(stale());
        }
        Ok(())
    }
}

fn stale() -> HostError {
    HostError::D3D8(D3D8Error32::StaleReply)
}
