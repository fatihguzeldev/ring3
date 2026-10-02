mod artifact;
mod cold;
mod gate;
mod region;
mod wasm;

pub(crate) use artifact::{compile_embedded_entry_region, compile_embedded_region};

pub use artifact::{
    ArtifactError, CompiledRegion, RegionMetadata, compile_entry_region, compile_region,
};
pub use cold::prepare_entry_region;
pub use gate::GateSpec;

pub use region::{
    BlockSpec, CompileError, CompileLimits, InstructionError, PreparedRegion, prepare_region,
};
