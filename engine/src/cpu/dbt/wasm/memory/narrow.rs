use wasm_encoder::{BlockType, InstructionSink, ValType};

use crate::cpu::x86::ir::{EffectiveAddress, SmallWidth};

use super::{
    Imports, add_check, effective_address, exit_if_failed, expect_field, helper_field,
    infrastructure, protocol_if_bad, reset_checks,
};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn load_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    let (length, import) = match width {
        SmallWidth::Byte => (1, imports.read8),
        SmallWidth::Word => (2, imports.read16),
    };
    effective_address(code, address);
    code.local_get(ADDRESS)
        .call(import.expect("prepared narrow read has an import"))
        .local_set(HELPER_STATUS);
    validate_result(code, length);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
}

fn validate_result(code: &mut InstructionSink<'_>, length: u32) {
    code.local_get(HELPER_STATUS).if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x0001_0002),
        (8, 40),
        (12, 0),
        (36, length),
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
    validate_success(code, length);
    code.else_()
        .local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    validate_fault(code, length);
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

fn validate_success(code: &mut InstructionSink<'_>, length: u32) {
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    helper_field(code, 20);
    code.i32_const(if length == 1 { 0xff } else { 0xffff })
        .i32_gt_u();
    add_check(code);
    code.local_get(ADDRESS)
        .i32_const(-(length as i32))
        .i32_gt_u();
    add_check(code);
    protocol_if_bad(code);
}

fn validate_fault(code: &mut InstructionSink<'_>, length: u32) {
    reset_checks(code);
    expect_field(code, 20, 0);
    expect_field(code, 32, 1);
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(2).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    code.local_get(ADDRESS)
        .i32_const(-(length as i32))
        .i32_le_u();
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_ne().i32_or().else_();
    code.local_get(ADDRESS)
        .i32_const(-(length as i32))
        .i32_gt_u();
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(length as i32 - 1)
        .i32_gt_u()
        .i32_or()
        .end();
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
