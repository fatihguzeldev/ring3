use super::{BATCH_HEADER_SIZE, GraphicsError, VERTEX_STRIDE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceId {
    pub epoch: u64,
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VertexFormat {
    ClipPositionColorRgba8 = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphicsLimits {
    pub resources: u32,
    pub resource_bytes: u32,
    pub batch_bytes: u32,
}

impl GraphicsLimits {
    /// provisioning ceilings bound eager bookkeeping and retained data, not gpu capabilities.
    pub const MAX_RESOURCES: u32 = 4096;
    pub const MAX_RESOURCE_BYTES: u32 = 256 * 1024 * 1024;
    pub const MAX_BATCH_BYTES: u32 = 16 * 1024 * 1024;
}

impl Default for GraphicsLimits {
    fn default() -> Self {
        Self {
            resources: 64,
            resource_bytes: 4 * 1024 * 1024,
            batch_bytes: 256 * 1024,
        }
    }
}

impl GraphicsLimits {
    pub(super) fn validate(self) -> Result<(), GraphicsError> {
        if self.resources == 0
            || self.resources > Self::MAX_RESOURCES
            || self.resource_bytes > Self::MAX_RESOURCE_BYTES
            || self.batch_bytes > Self::MAX_BATCH_BYTES
            || self.resource_bytes < VERTEX_STRIDE as u32
            || self.batch_bytes < (BATCH_HEADER_SIZE + 40 + VERTEX_STRIDE) as u32
        {
            return Err(GraphicsError::InvalidLimits);
        }
        Ok(())
    }
}

pub(super) struct Buffer {
    pub bytes: Vec<u8>,
    pub version: u64,
}

#[derive(Default)]
pub(super) struct Slot {
    pub generation: u32,
    pub buffer: Option<Buffer>,
}

pub(super) fn validate_vertices(bytes: &[u8]) -> Result<u32, GraphicsError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(VERTEX_STRIDE) {
        return Err(GraphicsError::InvalidVertexData);
    }
    let count =
        u32::try_from(bytes.len() / VERTEX_STRIDE).map_err(|_| GraphicsError::InvalidVertexData)?;
    for vertex in bytes.chunks_exact(VERTEX_STRIDE) {
        for (index, component) in vertex[..16].chunks_exact(4).enumerate() {
            let value = f32::from_le_bytes(component.try_into().unwrap());
            if !value.is_finite() || (index == 3 && value <= 0.0) {
                return Err(GraphicsError::InvalidVertexData);
            }
        }
    }
    Ok(count)
}
