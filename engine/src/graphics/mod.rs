#![forbid(unsafe_code)]

mod batch;
mod context;
mod resource;

pub use context::GraphicsContext;
pub use resource::{GraphicsLimits, ResourceId, VertexFormat};

pub const BATCH_HEADER_SIZE: usize = 40;
pub const VERTEX_STRIDE: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchId {
    pub epoch: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsError {
    InvalidLimits,
    InvalidEpoch,
    Busy,
    InvalidVertexData,
    InvalidColor,
    InvalidSurface,
    InvalidRange,
    StaleResource,
    StaleBatch,
    IdentityExhausted,
    ResourceCapacity,
    ByteCapacity,
    BatchCapacity,
    EmptyBatch,
    OutputTooSmall,
    Allocation,
}
