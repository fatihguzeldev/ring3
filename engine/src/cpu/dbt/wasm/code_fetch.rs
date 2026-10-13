use super::locals::{DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, REASON, STATE_PTR};
use crate::{abi::arena::SEGMENTS_OFFSET, cpu::x86::decode::DecodedInstruction};
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
    instruction: &DecodedInstruction,
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
    code.local_get(EIP).local_get(limit).i32_gt_u();
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
