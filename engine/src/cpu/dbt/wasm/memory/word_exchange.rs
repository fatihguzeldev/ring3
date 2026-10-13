use crate::abi::memory_helper::CpuScalarOperation;
use wasm_encoder::InstructionSink;

use crate::cpu::x86::ir::EffectiveAddress;

use super::{Imports, checked_address, exit_if_failed, helper_field};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        2,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .exchange16
        .expect("prepared word exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 2, CpuScalarOperation::Modify);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn compare_exchange_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        2,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .compare_exchange16
        .expect("prepared locked word compare exchange has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(register(crate::cpu::x86::Register32::Eax))
        .i32_const(0xffff)
        .i32_and()
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 2, CpuScalarOperation::Modify);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}

pub(super) fn exchange_add_result(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        2,
        crate::cpu::x86::SegmentAccess::Write,
        imports,
        exit_depth,
    );
    let index = imports
        .exchange_add16
        .expect("prepared locked word exchange add has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 2, CpuScalarOperation::Modify);
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
    code.local_set(RESULT);
}
