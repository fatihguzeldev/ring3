use crate::abi::memory_helper::CpuScalarOperation;
use crate::cpu::dbt::wasm::{
    helpers::protocol::{exit_if_failed, helper_field},
    imports::Imports,
    instructions::memory::checked_address,
    locals::*,
};
use crate::cpu::x86::{Register32, ir::EffectiveAddress};
use wasm_encoder::InstructionSink;
pub(in crate::cpu::dbt::wasm) fn bit_test_set_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::offset_value(code, address);
    code.local_get(register(source))
        .i32_const(5)
        .i32_shr_s()
        .i32_const(2)
        .i32_shl()
        .i32_add();
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        code.i32_const(0xffff).i32_and();
    }
    super::checked_offset(
        code,
        address.resolved_segment(),
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
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
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn bit_test_reset_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    super::offset_value(code, address);
    code.local_get(register(source))
        .i32_const(5)
        .i32_shr_s()
        .i32_const(2)
        .i32_shl()
        .i32_add();
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        code.i32_const(0xffff).i32_and();
    }
    super::checked_offset(
        code,
        address.resolved_segment(),
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
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
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_or_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_or_u32
        .expect("prepared locked memory or has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_and_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_and_u32
        .expect("prepared locked memory and has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_xor_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    mask: u32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_xor_u32
        .expect("prepared locked memory xor has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(mask as i32)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_or_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_or_u32
        .expect("prepared locked register memory or has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_and_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_and_u32
        .expect("prepared locked register memory and has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn atomic_xor_register_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .atomic_xor_u32
        .expect("prepared locked register memory xor has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn exchange_add_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .exchange_add32
        .expect("prepared locked memory exchange add has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .exchange
        .expect("prepared memory exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(in crate::cpu::dbt::wasm) fn compare_exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    source: Register32,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        4,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .compare_exchange32
        .expect("prepared locked memory compare exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(Register32::Eax))
        .local_get(register(source))
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(
        code,
        4,
        CpuScalarOperation::Modify,
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}
