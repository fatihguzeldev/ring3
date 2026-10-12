mod abi;
mod atomic;
mod cache_flush;
mod control;
mod control_instructions;
mod cpu_result;
mod dispatcher;
mod emitter;
mod integer;
mod integer_system;
mod locals;
mod memory;
mod packed_strings;
mod segments;
mod simd;
mod stack;
mod strings;
mod x87;

#[derive(Clone, Copy)]
pub(super) enum EmbeddedBinding {
    Replacement { key: u64, generation: u32 },
    Resident { key: u64, id: u64 },
}

pub(super) use emitter::emit;

pub(crate) use dispatcher::emit_dispatcher;
