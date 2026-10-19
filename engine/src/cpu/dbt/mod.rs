mod artifact;
mod cold;
mod gate;
mod region;
mod resident;
mod tier;
mod wasm;
mod x87_pair;

pub(crate) use x87_pair::{
    X87_PAIR_COMPLETED_SHIFT, X87_PAIR_REPORTED_SHIFT, X87PairProgress32, X87PairStep32, x87_pairs,
};

pub use tier::CompilationTier32;

pub(crate) use artifact::compile_runtime_entry;
pub(crate) use artifact::{compile_embedded_entry_region, compile_embedded_region};
pub(crate) use wasm::emit_dispatcher;
pub(crate) use wasm::emit_dispatcher_with_capacity;

pub use artifact::{
    ArtifactError, CompiledRegion, RegionMetadata, compile_entry_region, compile_region,
};
pub use cold::prepare_entry_region;
pub use gate::GateSpec;

pub use region::{
    BlockSpec, CompileError, CompileLimits, InstructionError, PreparedRegion, prepare_region,
};

pub(crate) use resident::{COHORT_OWNERS, ResidentCohort};

pub use resident::{RegistryError, RegistryLimits, RegistryUsage, ResidentRegistry, UnitId};
