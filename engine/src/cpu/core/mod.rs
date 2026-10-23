mod abi;
pub(crate) mod cold;
mod dispatch;
mod effects;
mod emitter;
mod flags;
mod memory;
mod program;
mod region;

pub(crate) const COLD_MARK: u32 = 0x4000_0000;
pub(crate) const STOP_MARK: u32 = 0x8000_0000;

pub use abi::{CONTEXT_SIZE, Context32, StopReason};
pub use dispatch::emit_dispatch;
pub use program::{
    CompileError, CompileOptions, CompiledCode, NativeInstruction, compile, compile_with_frontier,
    compile_with_frontier_query, compile_with_gates,
};
