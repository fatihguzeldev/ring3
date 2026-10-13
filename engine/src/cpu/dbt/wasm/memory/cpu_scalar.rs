use super::{add_check, expect_field, helper_field, infrastructure, protocol_if_bad, reset_checks};
use crate::abi::memory_helper::{CPU_SCALAR_HELPER_VERSION, CpuScalarOperation};
use crate::cpu::dbt::wasm::locals::*;
use wasm_encoder::{BlockType, InstructionSink};

/// current CPU scalar bus replies; no legacy packet version is accepted here.
pub(super) fn validate(code: &mut InstructionSink<'_>, width: u32, operation: CpuScalarOperation) {
    assert!(matches!(width, 1 | 2 | 4));
    code.local_get(HELPER_STATUS).i32_eqz();
    if operation != CpuScalarOperation::Read {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .i32_or();
    }
    code.i32_eqz().if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x0001_0000 | u32::from(CPU_SCALAR_HELPER_VERSION)),
        (8, 40),
        (12, 0),
        (36, width),
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
    success(code, width, operation);
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    fault(code, width, operation);
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    rejected(code, operation);
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end();
}

fn success(code: &mut InstructionSink<'_>, width: u32, operation: CpuScalarOperation) {
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    if operation == CpuScalarOperation::Store {
        expect_field(code, 20, 0);
    } else if width != 4 {
        helper_field(code, 20);
        code.i32_const(if width == 1 { 0xff } else { 0xffff })
            .i32_gt_u();
        add_check(code);
    }
    protocol_if_bad(code);
}

fn reject_invalidated_failure(code: &mut InstructionSink<'_>, operation: CpuScalarOperation) {
    if operation != CpuScalarOperation::Read {
        code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
        add_check(code);
    }
}

fn fault(code: &mut InstructionSink<'_>, width: u32, operation: CpuScalarOperation) {
    reset_checks(code);
    expect_field(code, 20, 0);
    reject_invalidated_failure(code, operation);
    match operation {
        CpuScalarOperation::Read => expect_field(code, 32, 1),
        CpuScalarOperation::Store => expect_field(code, 32, 2),
        CpuScalarOperation::Modify => {
            helper_field(code, 32);
            code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
            add_check(code);
            helper_field(code, 32);
            code.i32_const(2).i32_eq();
            helper_field(code, 24);
            code.i32_const(2).i32_ne().i32_and();
            add_check(code);
        }
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(width as i32)
        .i32_ge_u();
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

fn rejected(code: &mut InstructionSink<'_>, operation: CpuScalarOperation) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    reject_invalidated_failure(code, operation);
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
