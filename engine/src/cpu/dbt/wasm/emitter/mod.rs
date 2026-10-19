mod body;
mod module;

pub(in crate::cpu::dbt::wasm) use module::emit_profile;
pub(in crate::cpu::dbt) use module::{emit, emit_with_tier};
