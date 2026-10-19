#![forbid(unsafe_code)]

pub(super) mod avx;
pub(super) mod context;
pub(super) mod events;
pub(super) mod far;
pub(super) mod integer;
pub(super) mod segment_queries;
pub(super) mod segments;
pub(super) mod simd;
pub(super) mod stack;
pub(super) mod system_registers;
pub(super) mod x87;
pub(super) mod xstate;

mod fetch;
#[cfg(any(target_arch = "wasm32", test))]
pub(super) mod read_view;
