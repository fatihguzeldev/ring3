use wasm_encoder::InstructionSink;

use super::{TRANSACTION_VERSION, arg, load_field};

pub(in crate::cpu::core) const SEQUENCE: u32 = 99;

pub(in crate::cpu::core) fn load(code: &mut InstructionSink<'_>) {
    load_field(code, TRANSACTION_VERSION);
    code.i64_load(arg(0, 3)).local_set(SEQUENCE);
}

pub(in crate::cpu::core) fn flush(code: &mut InstructionSink<'_>) {
    load_field(code, TRANSACTION_VERSION);
    code.local_get(SEQUENCE).i64_store(arg(0, 3));
}
