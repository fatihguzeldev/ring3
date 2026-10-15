mod abi;
mod atomic;
mod avx;
mod cache_flush;
mod code_fetch;
mod control;
mod control_instructions;
mod helpers;
mod imports;

mod detection;
mod dispatcher;
mod emitter;
mod far;
mod integer;
mod integer_system;
mod locals;
mod memory;
mod packed_strings;
mod segment_queries;
mod segments;
mod simd;
mod software_interrupts;
mod stack;
mod strings;
mod system_registers;
mod x87;
mod xstate;

use helpers::result as cpu_result;

#[derive(Clone, Copy)]
pub(super) enum EmbeddedBinding {
    Replacement { key: u64, generation: u32 },
    Resident { key: u64, id: u64 },
}

pub(super) use emitter::emit;

pub(crate) use dispatcher::emit_dispatcher;
