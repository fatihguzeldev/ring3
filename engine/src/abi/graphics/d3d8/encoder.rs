use super::CodecError;
use crate::graphics::{
    ResourceId,
    d3d8::{
        commands::{ResourceVersion, SurfaceVersion, Targets},
        resources::Rect,
    },
};

pub(super) struct Encoder<'a> {
    output: Option<&'a mut Vec<u8>>,
    pub length: usize,
}

impl<'a> Encoder<'a> {
    pub fn new(output: Option<&'a mut Vec<u8>>) -> Self {
        Self { output, length: 0 }
    }
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        self.length = self
            .length
            .checked_add(bytes.len())
            .filter(|length| *length <= u32::MAX as usize)
            .ok_or(CodecError::Length)?;
        if let Some(output) = &mut self.output {
            output.extend_from_slice(bytes);
        }
        Ok(())
    }
    pub fn u32(&mut self, value: u32) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }
    pub fn u64(&mut self, value: u64) -> Result<(), CodecError> {
        self.bytes(&value.to_le_bytes())
    }
    pub fn words(&mut self, values: &[u32]) -> Result<(), CodecError> {
        for value in values {
            self.u32(*value)?;
        }
        Ok(())
    }
    pub fn count(&mut self, value: usize) -> Result<(), CodecError> {
        self.u32(u32::try_from(value).map_err(|_| CodecError::Length)?)
    }
    pub fn resource(&mut self, id: ResourceId) -> Result<(), CodecError> {
        if id.epoch == 0 || id.generation == 0 {
            return Err(CodecError::Identity);
        }
        self.u64(id.epoch)?;
        self.words(&[id.slot, id.generation])
    }
    pub fn version(&mut self, resource: ResourceVersion) -> Result<(), CodecError> {
        if resource.version == 0 {
            return Err(CodecError::Identity);
        }
        self.resource(resource.id)?;
        self.u64(resource.version)
    }
    pub fn surface(&mut self, surface: SurfaceVersion) -> Result<(), CodecError> {
        self.version(ResourceVersion {
            id: surface.surface.resource,
            version: surface.version,
        })?;
        self.words(&[surface.surface.subresource, 0])
    }
    pub fn targets(&mut self, targets: Targets) -> Result<(), CodecError> {
        self.surface(targets.color)?;
        self.u32(u32::from(targets.depth.is_some()))?;
        if let Some(depth) = targets.depth {
            self.surface(depth)?;
        }
        Ok(())
    }
    pub fn rect(&mut self, rect: Rect) -> Result<(), CodecError> {
        if rect.left >= rect.right || rect.top >= rect.bottom {
            return Err(CodecError::InvalidCommand);
        }
        self.words(&[rect.left, rect.top, rect.right, rect.bottom])
    }
}
