use crate::abi::memory_helper::CpuScalarOperation;
use wasm_encoder::InstructionSink;

use crate::cpu::x86::ir::EffectiveAddress;

use super::{Imports, checked_address, exit_if_failed};
use crate::cpu::dbt::wasm::locals::*;

pub(super) fn store_result(
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
    store_result_at_address(code, imports, exit_depth);
}

pub(super) fn store_result_at_address(
    code: &mut InstructionSink<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    let index = imports
        .store16
        .expect("prepared word store has an import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .local_get(RESULT)
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_scalar::validate(code, 2, CpuScalarOperation::Store);
    exit_if_failed(code, exit_depth, imports.raw_task_context.is_some());
}
