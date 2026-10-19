use super::{GroupLocals, protocol};
use crate::cpu::dbt::{
    X87_WIDE_GROUP_COMPLETED_SHIFT, X87_WIDE_GROUP_REPORTED_SHIFT, wasm::locals::HELPER_STATUS,
};
use wasm_encoder::{BlockType, InstructionSink, ValType};

pub(super) fn decode(code: &mut InstructionSink<'_>, locals: GroupLocals) {
    for (shift, mask, local) in [
        (X87_WIDE_GROUP_COMPLETED_SHIFT, 63, locals.completed),
        (X87_WIDE_GROUP_REPORTED_SHIFT, 31, locals.reported_step),
    ] {
        code.local_get(locals.progress)
            .i64_const(i64::from(shift))
            .i64_shr_u()
            .i32_wrap_i64()
            .i32_const(mask)
            .i32_and()
            .local_set(local);
    }
}

pub(super) fn check(
    code: &mut InstructionSink<'_>,
    locals: GroupLocals,
    length: u32,
    final_store: bool,
    exit_depth: u32,
) {
    protocol::reset_checks(code);
    code.local_get(locals.progress)
        .i64_const(43)
        .i64_shr_u()
        .i64_const(0)
        .i64_ne();
    protocol::add_check(code);
    code.local_get(locals.completed)
        .local_get(locals.limit)
        .i32_gt_u();
    protocol::add_check(code);
    code.local_get(locals.reported_step)
        .local_get(locals.limit)
        .i32_ge_u();
    protocol::add_check(code);
    code.local_get(locals.completed)
        .local_get(locals.reported_step)
        .i32_sub()
        .i32_const(1)
        .i32_gt_u();
    protocol::add_check(code);
    code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
    if final_store {
        code.if_(BlockType::Empty)
            .local_get(locals.completed)
            .i32_const(length as i32)
            .i32_ne()
            .local_get(locals.reported_step)
            .i32_const(length as i32 - 1)
            .i32_ne()
            .i32_or()
            .local_get(locals.limit)
            .i32_const(length as i32)
            .i32_ne()
            .i32_or();
        protocol::add_check(code);
        code.end();
    } else {
        protocol::add_check(code);
    }
    code.local_get(locals.completed)
        .local_get(locals.reported_step)
        .local_get(HELPER_STATUS)
        .i32_eqz();
    if final_store {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .i32_or();
    }
    code.if_(BlockType::Result(ValType::I32));
    protocol::helper_field(code, 16);
    code.i32_eqz().else_().i32_const(0).end().i32_add().i32_ne();
    protocol::add_check(code);
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}
