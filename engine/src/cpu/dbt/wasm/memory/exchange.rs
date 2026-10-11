use wasm_encoder::{BlockType, InstructionSink, ValType};

use crate::cpu::x86::{Register32, ir::EffectiveAddress};

use super::{
    Imports, add_check, effective_address, exit_if_failed, expect_field, helper_field,
    infrastructure, protocol_if_bad, reset_checks, validate_infrastructure, zero_fault_fields,
};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn bit_test_set_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::address_value(code, address);
    code.local_get(register(source))
        .i32_const(5)
        .i32_shr_s()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .local_set(ADDRESS);
    // retain the original selected bit beneath the checked helper's scratch operands.
    code.local_get(register(source))
        .i32_const(31)
        .i32_and()
        .local_tee(RESULT);
    let index = imports
        .bit_test_set32
        .expect("prepared locked memory bit set has an import")
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

pub(super) fn bit_test_reset_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::address_value(code, address);
    code.local_get(register(source))
        .i32_const(5)
        .i32_shr_s()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .local_set(ADDRESS);
    // retain the original selected bit beneath the checked helper's scratch operands.
    code.local_get(register(source))
        .i32_const(31)
        .i32_and()
        .local_tee(RESULT);
    let index = imports
        .bit_test_reset32
        .expect("prepared locked memory bit reset has an import")
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

pub(super) fn atomic_or_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_or_u32
        .expect("prepared locked memory or has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn atomic_and_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_and_u32
        .expect("prepared locked memory and has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn atomic_xor_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_xor_u32
        .expect("prepared locked memory xor has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn atomic_or_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_or_u32
        .expect("prepared locked register memory or has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn atomic_and_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_and_u32
        .expect("prepared locked register memory and has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn atomic_xor_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .atomic_xor_u32
        .expect("prepared locked register memory xor has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn exchange_add_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .exchange_add32
        .expect("prepared locked memory exchange add has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .exchange
        .expect("prepared memory exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    validate_result(code);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn compare_exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    effective_address(code, address);
    let index = imports
        .compare_exchange32
        .expect("prepared locked memory compare exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(Register32::Eax))
        .local_get(register(source))
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
        (4, 0x0001_0001),
        (8, 40),
        (12, 0),
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
    zero_fault_fields(code);
    code.local_get(ADDRESS).i32_const(-4).i32_gt_u();
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
    validate_infrastructure(code, true);
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end();
}

fn validate_fault(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    expect_field(code, 20, 0);
    expect_field(code, 36, 4);
    code.local_get(HELPER_STATUS).i32_const(11).i32_eq();
    add_check(code);
    helper_field(code, 32);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    helper_field(code, 32);
    code.i32_const(2).i32_eq();
    helper_field(code, 24);
    code.i32_const(2).i32_ne().i32_and();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(2).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    code.local_get(ADDRESS).i32_const(-4).i32_le_u();
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_ne().i32_or().else_();
    code.local_get(ADDRESS).i32_const(-4).i32_gt_u();
    helper_field(code, 28);
    code.i32_const(-4).i32_gt_u().i32_or();
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(3)
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
