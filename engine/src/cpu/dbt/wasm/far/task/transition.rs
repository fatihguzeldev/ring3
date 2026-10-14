use super::super::super::{abi, locals::*, memory};
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    next_pc: u32,
    imports: memory::Imports,
    depth: u32,
) {
    abi::flush_state(code);
    let helper = imports
        .task_return
        .expect("task transition helper")
        .emit_binding(code);
    code.i32_const(next_pc as i32)
        .call(helper)
        .local_set(HELPER_STATUS);
    // a committed switch owns these values even when the helper rejects later currentness.
    abi::load_state(code);
    abi::validate_loaded_state(code);
    checked(code, depth);
    memory::reset_checks(code);
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x1000e),
        (8, 40),
        (12, 0),
    ] {
        memory::expect_field(code, offset, value);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    code.else_();
    memory::infrastructure(code, 2);
    code.end();
    code.br(depth + 1).end();
    memory::helper_field(code, 16);
    code.local_set(RHS)
        .local_get(RHS)
        .i32_eqz()
        .if_(BlockType::Empty);
    committed(code, imports, depth + 1);
    code.else_();
    code.local_get(HELPER_STATUS).if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    code.br(depth + 2).end();
    code.local_get(RHS)
        .i32_const(4)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    fault_fields(code, false);
    for offset in [28, 32, 36] {
        memory::expect_field(code, offset, 0);
    }
    checked(code, depth + 2);
    memory::helper_field(code, 20);
    code.local_set(REASON);
    memory::helper_field(code, 24);
    code.local_set(DETAIL);
    code.else_()
        .local_get(RHS)
        .i32_const(8)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    for offset in [28, 32, 36] {
        memory::expect_field(code, offset, 0);
    }
    memory::helper_field(code, 20);
    code.local_set(RESULT);
    code.local_get(RESULT)
        .i32_const(1)
        .i32_sub()
        .i32_const(9)
        .i32_gt_u();
    memory::add_check(code);
    memory::helper_field(code, 24);
    code.local_set(ADDRESS);
    code.local_get(RESULT)
        .i32_const(5)
        .i32_eq()
        .local_get(RESULT)
        .i32_const(6)
        .i32_eq()
        .i32_or()
        .local_get(RESULT)
        .i32_const(10)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(ADDRESS).i32_const(0xffff).i32_gt_u();
    memory::add_check(code);
    code.else_()
        .local_get(RESULT)
        .i32_const(7)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(ADDRESS).i32_const(3).i32_gt_u();
    memory::add_check(code);
    code.else_()
        .local_get(RESULT)
        .i32_const(9)
        .i32_ne()
        .if_(BlockType::Empty);
    memory::expect_field(code, 24, 0);
    code.end().end().end();
    checked(code, depth + 3);
    neutral(code, imports);
    code.i32_const(4)
        .local_set(REASON)
        .i32_const(4)
        .local_set(DETAIL);
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    for offset in [24, 32] {
        memory::helper_field(code, offset);
        code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
        memory::add_check(code);
    }
    memory::helper_field(code, 36);
    code.local_set(RESULT);
    code.local_get(RESULT)
        .i32_const(2)
        .i32_ne()
        .local_get(RESULT)
        .i32_const(44)
        .i32_ne()
        .i32_and()
        .local_get(RESULT)
        .i32_const(104)
        .i32_ne()
        .i32_and();
    memory::add_check(code);
    memory::helper_field(code, 32);
    code.i32_const(2)
        .i32_eq()
        .local_get(RESULT)
        .i32_const(2)
        .i32_ne()
        .i32_and();
    memory::add_check(code);
    checked(code, depth + 4);
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        memory::helper_field(code, offset);
        code.local_set(local);
    }
    code.i32_const(5).local_set(REASON);
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    memory::reset_checks(code);
    for offset in [20, 28, 32, 36] {
        memory::expect_field(code, offset, 0);
    }
    memory::helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    memory::add_check(code);
    checked(code, depth + 5);
    memory::helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    memory::infrastructure(code, 1);
    code.else_();
    memory::infrastructure(code, 3);
    code.end().else_();
    memory::infrastructure(code, 2);
    code.end().end().end().end().end();
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.local_get(RHS)
            .i32_const(1)
            .i32_ne()
            .if_(BlockType::Empty)
            .i32_const(0)
            .local_set(local)
            .end();
    }
}

fn committed(code: &mut InstructionSink<'_>, imports: memory::Imports, depth: u32) {
    memory::reset_checks(code);
    fault_fields(code, true);
    for offset in [28, 36] {
        memory::expect_field(code, offset, 0);
    }
    memory::helper_field(code, 32);
    code.local_set(ADDRESS)
        .local_get(ADDRESS)
        .i32_const(!3)
        .i32_and();
    memory::add_check(code);
    code.local_get(ADDRESS)
        .i32_const(1)
        .i32_and()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .i32_ne();
    memory::add_check(code);
    checked(code, depth);
    code.local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED)
        .local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .i32_const(0)
        .local_set(SNAPSHOT_TF);
    code.i32_const(11)
        .i32_const(0)
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .select()
        .local_set(
            imports
                .task_return_transport
                .expect("task transition transport"),
        );
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    memory::infrastructure(code, 3);
    code.else_();
    memory::helper_field(code, 20);
    code.local_set(RESULT)
        .local_get(RESULT)
        .i32_eqz()
        .if_(BlockType::Empty);
    code.local_get(ADDRESS)
        .i32_const(2)
        .i32_and()
        .if_(BlockType::Empty);
    // the task t-bit needs a live debug-status owner before it can become a debug exception.
    code.i32_const(4)
        .local_set(REASON)
        .i32_const(6)
        .local_set(DETAIL);
    code.else_()
        .i32_const(3)
        .local_set(REASON)
        .i32_const(0)
        .local_set(DETAIL)
        .end();
    code.else_().local_get(RESULT).local_set(REASON);
    memory::helper_field(code, 24);
    code.local_set(DETAIL);
    code.end().end();
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
}

fn fault_fields(code: &mut InstructionSink<'_>, committed: bool) {
    memory::helper_field(code, 20);
    code.local_set(RESULT)
        .local_get(RESULT)
        .i32_const(14)
        .i32_ne()
        .local_get(RESULT)
        .i32_const(21)
        .i32_ne()
        .i32_and();
    if committed {
        code.local_get(RESULT)
            .i32_const(11)
            .i32_ne()
            .i32_and()
            .local_get(RESULT)
            .i32_const(15)
            .i32_ne()
            .i32_and()
            .local_get(RESULT)
            .i32_eqz()
            .i32_eqz()
            .i32_and();
    }
    memory::add_check(code);
    memory::helper_field(code, 24);
    code.local_set(RESULT)
        .local_get(RESULT)
        .i32_const(0xfffc)
        .i32_gt_u()
        .local_get(RESULT)
        .i32_const(3)
        .i32_and()
        .i32_or();
    memory::add_check(code);
    memory::helper_field(code, 20);
    code.i32_const(14).i32_eq().if_(BlockType::Empty);
    code.local_get(RESULT).i32_eqz();
    memory::add_check(code);
    if !committed {
        code.local_get(RESULT).i32_const(4).i32_and();
        memory::add_check(code);
    }
    code.end();
    if committed {
        memory::helper_field(code, 20);
        code.i32_const(15).i32_eq().if_(BlockType::Empty);
        code.local_get(RESULT).i32_eqz();
        memory::add_check(code);
        code.end();
        memory::helper_field(code, 20);
        code.i32_const(11).i32_eq();
        memory::helper_field(code, 20);
        code.i32_eqz().i32_or().if_(BlockType::Empty);
        memory::expect_field(code, 24, 0);
        code.end();
    }
}

fn neutral(code: &mut InstructionSink<'_>, imports: memory::Imports) {
    code.local_get(imports.task_return_flags.expect("original task flags"))
        .local_set(FLAGS)
        .local_get(SNAPSHOT_SHADOW)
        .local_set(DEBUG_SHADOW);
}

fn checked(code: &mut InstructionSink<'_>, depth: u32) {
    memory::protocol_if_bad(code);
    // the old module interpretation is obsolete after a committed task switch.
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .br(depth + 1)
        .end();
}
