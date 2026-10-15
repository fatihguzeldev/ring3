mod blocks;
mod context;
mod render;
mod types;

pub use blocks::{StateBlock, StateBlockKind};
pub use context::{DeviceState, PipelineState};
pub use types::{Light, LightState, Material, Matrix, StreamBinding, Viewport};

pub const TEXTURE_STAGES: usize = 8;
pub const VERTEX_STREAMS: usize = 16;
pub const VERTEX_CONSTANTS: usize = 96;
pub const PIXEL_CONSTANTS: usize = 8;
pub const CLIP_PLANES: usize = 6;
pub const MAX_LIGHTS: usize = 256;
pub const MAX_ACTIVE_LIGHTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    InvalidState,
    InvalidValue,
    InvalidRange,
    InvalidLight,
    LightCapacity,
    Recording,
    NotRecording,
    IdentityExhausted,
    Allocation,
}

#[cfg(test)]
mod tests;
