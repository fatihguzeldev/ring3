use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        dbt::wasm::{helpers::protocol, imports::Imports, locals::*},
        x86::Register32,
    },
};
use wasm_encoder::{InstructionSink, MemArg};

pub(in crate::cpu::dbt::wasm) fn transfer(offset: usize) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET + offset) as u64,
        align: 2,
        memory_index: 0,
    }
}
pub(in crate::cpu::dbt::wasm) fn payload(code: &mut InstructionSink<'_>) {
    for (i, r) in [
        Register32::Eax,
        Register32::Edx,
        Register32::Ebx,
        Register32::Ecx,
    ]
    .into_iter()
    .enumerate()
    {
        code.local_get(STATE_PTR)
            .local_get(register(r))
            .i32_store(transfer(8 + i * 4));
    }
    code.local_get(STATE_PTR)
        .local_get(FLAGS)
        .i32_store(transfer(4));
}
pub(in crate::cpu::dbt::wasm) fn call(
    code: &mut InstructionSink<'_>,
    descriptor: u32,
    width: u32,
    imports: Imports,
    exit_depth: u32,
) {
    let index = imports
        .cpu_integer
        .expect("integer helper")
        .emit_binding(code);
    code.i32_const(descriptor as i32)
        .local_get(ADDRESS)
        .call(index)
        .local_set(HELPER_STATUS);
    if matches!(descriptor, 0x30001..=0x30004) {
        crate::cpu::dbt::wasm::code_fetch::check_context(code, exit_depth);
    }
    crate::cpu::dbt::wasm::helpers::result::validate(
        code,
        width,
        if width == 0 { 0 } else { 3 },
        width != 0,
        exit_depth,
        if matches!(descriptor, 0x30001..=0x30004) {
            crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports).read_intent()
        } else {
            crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports).write_intent()
        },
    );
    protocol::reset_checks(code);
    protocol::expect_field(code, 20, 0);
    protocol::protocol_if_bad(code);
    protocol::exit_if_failed(code, exit_depth);
}
