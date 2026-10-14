mod abi;
mod atomic;
mod avx;
mod cache_flush;
mod code_fetch;
mod control;
mod control_instructions;
mod cpu_result;
mod detection;
mod dispatcher;
mod emitter;
mod far;
mod integer;
mod integer_system;
mod interrupts;
mod locals;
mod memory;
mod packed_strings;
mod segment_queries;
mod segments;
mod simd;
mod stack;
mod strings;
mod system_registers;
mod x87;
mod xstate;

#[derive(Clone, Copy)]
pub(super) enum EmbeddedBinding {
    Replacement { key: u64, generation: u32 },
    Resident { key: u64, id: u64 },
}

#[cfg(test)]
pub(super) use emitter::emit;
pub(super) use emitter::emit_with_authority;

pub(crate) use dispatcher::emit_dispatcher;

mod tasks;
