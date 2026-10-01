mod artifact;
mod region;
mod wasm;

pub(crate) use artifact::compile_embedded_region;

pub use artifact::{ArtifactError, CompiledRegion, RegionMetadata, compile_region};

pub use region::{
    BlockSpec, CompileError, CompileLimits, InstructionError, PreparedRegion, prepare_region,
};
