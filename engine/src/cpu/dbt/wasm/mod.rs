mod abi;
mod control;
mod dispatcher;
mod emitter;
mod integer;
mod locals;
mod memory;
mod x87;

#[derive(Clone, Copy)]
pub(super) enum EmbeddedBinding {
    Replacement { key: u64, generation: u32 },
    Resident { key: u64, id: u64 },
}

pub(super) use emitter::emit;

pub(crate) use dispatcher::emit_dispatcher;
