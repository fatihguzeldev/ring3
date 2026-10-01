mod abi;
mod control;
mod emitter;
mod integer;
mod locals;

#[derive(Clone, Copy)]
pub(super) struct EmbeddedBinding {
    pub(super) key: u64,
    pub(super) generation: u32,
}

pub(super) use emitter::emit;
