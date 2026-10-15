use crate::abi::memory_helper::CpuScalarOperation;
use crate::cpu::dbt::wasm::{
    helpers::protocol::{exit_if_failed, helper_field},
    imports::Imports,
    instructions::memory::checked_address,
    locals::*,
};
use crate::cpu::x86::ir::{EffectiveAddress, SmallWidth};
use wasm_encoder::InstructionSink;
pub(in crate::cpu::dbt::wasm) fn load_value(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    checked_address(
        code,
        address,
        match width {
            SmallWidth::Byte => 1,
            SmallWidth::Word => 2,
        },
        crate::cpu::x86::SegmentAccess::Read,
        imports,
        exit_depth,
    );
    load_at_address(code, width, imports, exit_depth);
}

pub(in crate::cpu::dbt::wasm) fn load_at_address(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
) {
    load_at_address_with_intent(code, width, imports, exit_depth, false);
}

pub(super) fn load_at_address_with_intent(
    code: &mut InstructionSink<'_>,
    width: SmallWidth,
    imports: Imports,
    exit_depth: u32,
    modify: bool,
) {
    let paging_modify = modify && imports.paging_configured;
    let (length, import) = match width {
        SmallWidth::Byte => (
            1,
            if paging_modify {
                imports.read_modify8
            } else {
                imports.read8
            },
        ),
        SmallWidth::Word => (
            2,
            if paging_modify {
                imports.read_modify16
            } else {
                imports.read16
            },
        ),
    };
    code.local_get(ADDRESS)
        .call(import.expect("prepared narrow read has an import"))
        .local_set(HELPER_STATUS);
    crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
    super::cpu_scalar::validate(
        code,
        length,
        if paging_modify {
            CpuScalarOperation::Modify
        } else {
            CpuScalarOperation::Read
        },
        imports.paging_configured,
    );
    exit_if_failed(code, exit_depth);
    helper_field(code, 20);
}
