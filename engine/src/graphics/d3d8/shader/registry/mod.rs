mod context;
mod objects;

pub use context::{PreparedShader, ShaderRegistry};
pub use objects::{PixelShader, VertexShader};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShaderLimits {
    pub objects: u32,
    pub bytes: u32,
}

impl Default for ShaderLimits {
    fn default() -> Self {
        Self {
            objects: 1024,
            bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryError {
    Shader(super::ShaderError),
    InvalidLimits,
    InvalidUsage,
    InvalidHandle,
    ObjectCapacity,
    ByteCapacity,
    IdentityExhausted,
    Allocation,
}

impl From<super::ShaderError> for RegistryError {
    fn from(error: super::ShaderError) -> Self {
        Self::Shader(error)
    }
}

pub fn is_shader_handle(handle: u32) -> bool {
    handle & 0x8000_0001 == 0x8000_0001
}

#[cfg(test)]
mod tests;
