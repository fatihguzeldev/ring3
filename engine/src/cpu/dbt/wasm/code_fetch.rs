use crate::{
    abi::arena::SEGMENTS_OFFSET,
    cpu::{
        dbt::wasm::locals::{
            DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, HELPER_STATUS, REASON,
            STATE_PTR,
        },
        x86::decode::DecodedOperation,
    },
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};
pub(super) fn refresh(code: &mut InstructionSink<'_>, limit: u32) {
    code.local_get(STATE_PTR)
        .i32_load16_u(arg(SEGMENTS_OFFSET + 4, 1))
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(-1)
        .else_();
    code.local_get(STATE_PTR)
        .i32_load(arg(SEGMENTS_OFFSET + 32, 2))
        .local_tee(limit)
        .i32_const(1 << 24)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    code.local_get(limit)
        .i32_const(0xfffff)
        .i32_and()
        .i32_const(12)
        .i32_shl()
        .i32_const(0xfff)
        .i32_or();
    code.else_()
        .local_get(limit)
        .i32_const(0xfffff)
        .i32_and()
        .end()
        .end()
        .local_set(limit);
}

pub(super) fn check_instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    limit: u32,
    depth: u32,
) {
    check_span(code, instruction.pc().0, instruction.length(), limit, depth);
}

pub(super) fn check_span(
    code: &mut InstructionSink<'_>,
    pc: u32,
    length: u8,
    limit: u32,
    depth: u32,
) {
    if let Some(last) = pc.checked_add(u32::from(length) - 1) {
        code.i32_const(last as i32).local_get(limit).i32_gt_u();
    } else {
        code.i32_const(1);
    }
    fault_if(code, depth);
}

pub(super) fn check_pc(code: &mut InstructionSink<'_>, limit: u32, depth: u32) {
    check_value(code, EIP, limit, depth);
}

pub(super) fn check_value(code: &mut InstructionSink<'_>, value: u32, limit: u32, depth: u32) {
    code.local_get(value).local_get(limit).i32_gt_u();
    fault_if(code, depth);
}

fn fault_if(code: &mut InstructionSink<'_>, depth: u32) {
    code.if_(BlockType::Empty).i32_const(11).local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(depth + 1).end();
}

fn arg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: offset as u64,
        align,
        memory_index: 0,
    }
}

// module-private immutable globals hold the retained interpretation, never live authority.
pub(super) fn check_context(code: &mut InstructionSink<'_>, depth: u32) {
    code.global_get(0).if_(BlockType::Empty);
    code.local_get(STATE_PTR)
        .i32_load16_u(arg(SEGMENTS_OFFSET + 4, 1))
        .i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .local_get(STATE_PTR)
        .i32_load(arg(crate::abi::arena::SEGMENT_BASES_RECORD_OFFSET + 20, 2))
        .else_()
        .i32_const(0)
        .end()
        .global_get(1)
        .i32_ne();
    code.local_get(STATE_PTR)
        .i32_load16_u(arg(SEGMENTS_OFFSET + 4, 1))
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_()
        .local_get(STATE_PTR)
        .i32_load(arg(SEGMENTS_OFFSET + 32, 2))
        .i32_const(1 << 25)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .end()
        .global_get(2)
        .i32_ne()
        .i32_or()
        .if_(BlockType::Empty);
    code.local_get(EIP);
    live_limit(code);
    code.i32_gt_u()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(11)
        .else_()
        .i32_const(3)
        .end()
        .local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(depth + 2).end().end();
}

pub(super) fn check_rejected(code: &mut InstructionSink<'_>, depth: u32) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    check_context(code, depth + 1);
    code.end();
}

fn live_limit(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i32_load16_u(arg(SEGMENTS_OFFSET + 4, 1))
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(-1)
        .else_();
    code.local_get(STATE_PTR)
        .i32_load(arg(SEGMENTS_OFFSET + 32, 2))
        .i32_const(1 << 24)
        .i32_and()
        .if_(BlockType::Result(ValType::I32));
    code.local_get(STATE_PTR)
        .i32_load(arg(SEGMENTS_OFFSET + 32, 2))
        .i32_const(0xfffff)
        .i32_and()
        .i32_const(12)
        .i32_shl()
        .i32_const(0xfff)
        .i32_or();
    code.else_()
        .local_get(STATE_PTR)
        .i32_load(arg(SEGMENTS_OFFSET + 32, 2))
        .i32_const(0xfffff)
        .i32_and()
        .end()
        .end();
}

mod hoist;
pub(super) use hoist::refresh_after;

mod paging;
pub(super) use paging::fault as paging_fault;
