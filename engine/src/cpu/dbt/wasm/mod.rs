mod abi;
mod code_fetch;
mod helpers;
mod imports;
mod instructions;

mod detection;
mod dispatcher;
mod emitter;
mod locals;
mod native_success;
mod requirements;

#[derive(Clone, Copy)]
pub(super) enum EmbeddedBinding {
    Replacement { key: u64, generation: u32 },
    Resident { key: u64, id: u64 },
}

pub(super) use emitter::{emit, emit_with_tier};
pub(super) use native_success::emit as emit_native_success;

pub(crate) use dispatcher::emit_dispatcher;
pub(crate) use dispatcher::emit_dispatcher_with_capacity;
