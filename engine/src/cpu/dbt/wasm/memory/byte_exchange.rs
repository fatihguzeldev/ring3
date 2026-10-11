use wasm_encoder::{BlockType, InstructionSink};

use crate::cpu::x86::ir::EffectiveAddress;

use super::{
    Imports, add_check, effective_address, exit_if_failed, expect_field, helper_field,
    infrastructure, protocol_if_bad, reset_checks,
};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .exchange8
        .expect("prepared byte exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

fn validate_result(code: &mut InstructionSink<'_>) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .i32_or()
        .i32_eqz()
        .if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x0001_0005),
        (8, 40),
        (12, 0),
        (36, 1),
    ] {
        expect_field(code, offset, expected);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 16);
    code.local_set(RHS)
        .local_get(RHS)
        .i32_eqz()
        .if_(BlockType::Empty);
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    helper_field(code, 20);
    code.i32_const(0xff).i32_gt_u();
    add_check(code);
    protocol_if_bad(code);
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    validate_fault(code);
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    validate_infrastructure(code);
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end();
}

fn validate_fault(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    expect_field(code, 20, 0);
    code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
    add_check(code);
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_ne();
    add_check(code);
    for offset in [24, 32] {
        helper_field(code, offset);
        code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
        add_check(code);
    }
    helper_field(code, 32);
    code.i32_const(2).i32_eq();
    helper_field(code, 24);
    code.i32_const(2).i32_ne().i32_and();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_().i32_const(5).local_set(REASON);
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        helper_field(code, offset);
        code.local_set(local);
    }
    code.end();
}

fn validate_infrastructure(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    infrastructure(code, 1);
    code.else_();
    infrastructure(code, 3);
    code.end().end();
}
