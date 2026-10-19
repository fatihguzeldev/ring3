use super::{
    super::pair::{PacketContract, select_step, validate_packet},
    GroupLocals,
};
use crate::cpu::dbt::{
    X87_WIDE_GROUP_MAX_STEPS, X87PairStep32, X87WideGroup32,
    wasm::{helpers::protocol, imports::Imports, locals::*},
};
use wasm_encoder::{BlockType, InstructionSink, ValType};

mod packets;
mod progress;

pub(in crate::cpu::dbt::wasm) const INTEGER_LOCALS: u32 = 36;

fn descriptor(group: X87WideGroup32<'_>, index: usize) -> X87PairStep32 {
    group
        .descriptor(index)
        .expect("planned wide x87 descriptor")
}

pub(in crate::cpu::dbt::wasm) fn capture_limit(
    code: &mut InstructionSink<'_>,
    group: X87WideGroup32<'_>,
    imports: Imports,
) {
    let locals = imports.x87_group_locals.expect("wide x87 group locals");
    code.local_get(BUDGET)
        .i32_const(group.len() as i32)
        .i32_lt_u()
        .if_(BlockType::Result(ValType::I32))
        .local_get(BUDGET)
        .else_()
        .i32_const(group.len() as i32)
        .end()
        .local_set(locals.limit)
        .local_get(FLAGS)
        .i32_const(0x10100)
        .i32_and()
        .local_get(DEBUG_SHADOW)
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(locals.limit)
        .end();
}

pub(in crate::cpu::dbt::wasm) fn operation(
    code: &mut InstructionSink<'_>,
    ordinal: u32,
    group: X87WideGroup32<'_>,
    imports: Imports,
    exit_depth: u32,
) {
    let locals = imports.x87_group_locals.expect("wide x87 group locals");
    for index in 0..group.len() {
        if let Some(address) = descriptor(group, index).address {
            crate::cpu::dbt::wasm::instructions::memory::address_value(code, address);
        } else {
            code.i32_const(0);
        }
        code.local_set(locals.address(index));
    }
    let import = imports.x87_group32.expect("resident wide x87 group helper");
    let import = import.emit_binding(code);
    code.i32_const(ordinal as i32);
    for index in 0..X87_WIDE_GROUP_MAX_STEPS {
        if index < group.len() {
            code.local_get(locals.address(index));
        } else {
            code.i32_const(0);
        }
    }
    code.local_get(FLAGS)
        .local_get(locals.limit)
        .call(import)
        .local_tee(locals.progress)
        .i32_wrap_i64()
        .local_set(HELPER_STATUS);
    progress::decode(code, locals);
    let final_store = descriptor(group, group.len() - 1).operation.write_size() != 0;
    progress::check(code, locals, group.len() as u32, final_store, exit_depth);

    // retain the reported boundary for faults, and its next pc only for a
    // fully validated success whose reported instruction completed.
    code.block(BlockType::Empty);
    for index in 0..group.len() {
        let step = descriptor(group, index);
        code.local_get(locals.reported_step)
            .i32_const(index as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        select_step(code, step, locals.address(index));
        code.i32_const(step.next_pc as i32)
            .local_set(locals.next_pc)
            .br(1)
            .end();
    }
    code.end();
    packets::validate(code, group, locals, imports);
    code.local_get(REASON)
        .if_(BlockType::Empty)
        .local_get(locals.reported_step)
        .local_set(locals.completed)
        .else_()
        .local_get(locals.next_pc)
        .local_set(EIP)
        .end()
        .local_get(RETIRED)
        .local_get(locals.completed)
        .i32_add()
        .local_set(RETIRED)
        .local_get(BUDGET)
        .local_get(locals.completed)
        .i32_sub()
        .local_set(BUDGET);
    protocol::exit_if_failed(code, exit_depth);
    if final_store {
        protocol::exit_if_invalidated(code, exit_depth, imports);
    }
}
