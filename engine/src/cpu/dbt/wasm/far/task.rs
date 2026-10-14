use super::super::{code_fetch, locals::*, memory};
use wasm_encoder::{BlockType, InstructionSink};

pub(in crate::cpu::dbt::wasm) fn return_if_nested(
    code: &mut InstructionSink<'_>,
    next_pc: u32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    code.local_get(FLAGS)
        .i32_const(0x4000)
        .i32_and()
        .if_(BlockType::Empty);
    if imports.task_transition_configured {
        transition::emit(code, next_pc, imports, exit_depth + 1);
        code.br(exit_depth + 1).end();
        return;
    }
    let helper = imports
        .task_return
        .expect("task admission helper")
        .emit_binding(code);
    code.call(helper).local_set(HELPER_STATUS);
    code_fetch::check_context(code, exit_depth + 1, imports.raw_task_context.is_some());
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    memory::exit_if_failed(code, exit_depth + 2, imports.raw_task_context.is_some());
    code.end();
    memory::reset_checks(code);
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x1000d),
        (8, 40),
        (12, 0),
        (20, 0),
    ] {
        memory::expect_field(code, offset, value);
    }
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth + 1, imports.raw_task_context.is_some());
    validate(
        code,
        imports.task_return_flags.expect("original task flags"),
    );
    code.br(exit_depth + 1).end();
}

mod transition;

fn validate(code: &mut InstructionSink<'_>, original_flags: u32) {
    memory::helper_field(code, 16);
    code.local_set(RHS);
    code.local_get(RHS)
        .i32_const(8)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    for offset in [32, 36] {
        memory::expect_field(code, offset, 0);
    }
    memory::helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    memory::helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    memory::expect_field(code, 28, 0);
    code.else_();
    memory::helper_field(code, 28);
    code.i32_const(0xffff).i32_gt_u();
    memory::add_check(code);
    memory::helper_field(code, 28);
    code.i32_const(!3).i32_and().i32_eqz();
    memory::add_check(code);
    memory::helper_field(code, 28);
    code.i32_const(4).i32_and();
    memory::add_check(code);
    code.end();
    code.local_get(LHS).if_(BlockType::Empty);
    memory::infrastructure(code, 2);
    code.else_();
    code.i32_const(4)
        .local_set(REASON)
        .i32_const(4)
        .local_set(DETAIL)
        .local_get(original_flags)
        .local_set(FLAGS)
        .local_get(SNAPSHOT_SHADOW)
        .local_set(DEBUG_SHADOW);
    code.end();
    code.else_()
        .local_get(RHS)
        .i32_const(3)
        .i32_eq()
        .local_get(RHS)
        .i32_const(6)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    for offset in [28, 32, 36] {
        memory::expect_field(code, offset, 0);
    }
    memory::helper_field(code, 24);
    code.i32_const(0xfffc).i32_gt_u();
    memory::add_check(code);
    memory::helper_field(code, 24);
    code.i32_const(3).i32_and();
    memory::add_check(code);
    code.local_get(RHS)
        .i32_const(6)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::helper_field(code, 24);
    code.i32_eqz();
    memory::add_check(code);
    memory::helper_field(code, 24);
    code.i32_const(4).i32_and();
    memory::add_check(code);
    code.end();
    code.local_get(LHS).if_(BlockType::Empty);
    memory::infrastructure(code, 2);
    code.else_();
    code.i32_const(21)
        .i32_const(14)
        .local_get(RHS)
        .i32_const(3)
        .i32_eq()
        .select()
        .local_set(REASON);
    memory::helper_field(code, 24);
    code.local_set(DETAIL);
    code.end();
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    memory::expect_field(code, 32, 1);
    memory::expect_field(code, 36, 2);
    memory::helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    memory::infrastructure(code, 2);
    code.else_();
    code.i32_const(5).local_set(REASON);
    memory::helper_field(code, 24);
    code.local_set(DETAIL);
    memory::helper_field(code, 28);
    code.local_set(FAULT_ADDRESS);
    code.i32_const(1)
        .local_set(FAULT_ACCESS)
        .i32_const(2)
        .local_set(FAULT_LENGTH);
    code.end();
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    memory::expect_field(code, 28, 0);
    memory::expect_field(code, 32, 0);
    memory::expect_field(code, 36, 2);
    memory::helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    memory::infrastructure(code, 2);
    code.else_();
    memory::helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    memory::infrastructure(code, 1);
    code.else_();
    memory::infrastructure(code, 3);
    code.end().end();
    code.else_();
    memory::infrastructure(code, 2);
    code.end().end().end().end();
}
