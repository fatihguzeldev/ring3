mod abi;
pub(crate) mod cold;
pub(crate) mod continuation;
mod dispatch;
mod effects;
mod emitter;
mod flags;
pub(crate) mod fp_pool;
mod memory;
mod program;
mod region;
pub(crate) mod trace;

pub(crate) const COLD_MARK: u32 = 0x4000_0000;
pub(crate) const STOP_MARK: u32 = 0x8000_0000;

pub use abi::{CONTEXT_SIZE, Context32, StopReason};
pub use dispatch::emit_dispatch;
pub(crate) use effects::numeric_preserves_fp;
pub use program::{
    CompileError, CompileOptions, CompiledCode, NativeInstruction, compile, compile_with_frontier,
    compile_with_frontier_query, compile_with_gates,
};
pub(crate) use program::{
    ExecutionCandidate, ObservedPlan, ObservedPlanFingerprint, WindowFingerprint, WindowPlan,
    emit_hot_window, emit_observed_plan, for_each_execution_candidate, prepare_cold_prefix_plan,
    prepare_hot_window, prepare_observed_plan,
};
pub(crate) use program::{
    HotSpan, TraceScore, compile_observed, compile_observed_with_cold, compile_observed_with_growth,
};
