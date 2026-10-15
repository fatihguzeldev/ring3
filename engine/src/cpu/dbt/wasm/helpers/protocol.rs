use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::cpu::dbt::wasm::locals::*;

pub(in crate::cpu::dbt::wasm) fn infrastructure(code: &mut InstructionSink<'_>, detail: i32) {
    code.i32_const(7)
        .local_set(REASON)
        .i32_const(detail)
        .local_set(DETAIL);
}

pub(in crate::cpu::dbt::wasm) fn protocol_if_bad(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.end();
}

pub(in crate::cpu::dbt::wasm) fn exit_if_failed(code: &mut InstructionSink<'_>, exit_depth: u32) {
    crate::cpu::dbt::wasm::code_fetch::check_rejected(code, exit_depth);
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .br(exit_depth + 1)
        .end();
}

pub(in crate::cpu::dbt::wasm) fn reset_checks(code: &mut InstructionSink<'_>) {
    code.i32_const(0).local_set(LHS);
}

pub(in crate::cpu::dbt::wasm) fn expect_field(
    code: &mut InstructionSink<'_>,
    offset: u64,
    expected: u32,
) {
    helper_field(code, offset);
    code.i32_const(expected as i32).i32_ne();
    add_check(code);
}

pub(in crate::cpu::dbt::wasm) fn add_check(code: &mut InstructionSink<'_>) {
    code.local_get(LHS).i32_or().local_set(LHS);
}

pub(in crate::cpu::dbt::wasm) fn helper_field(code: &mut InstructionSink<'_>, offset: u64) {
    code.local_get(STATE_PTR).i32_load(MemArg {
        offset: 100 + offset,
        align: 2,
        memory_index: 0,
    });
}
