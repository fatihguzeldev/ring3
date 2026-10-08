use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::{
    abi::{
        ABI_VERSION, X86_INTEGER_PROFILE,
        arena::X87_OFFSET,
        x86::{
            EXIT_SIZE, STATE_SIZE, X87_CODE_SELECTOR_OFFSET, X87_CONTROL_OFFSET,
            X87_DATA_POINTER_OFFSET, X87_INSTRUCTION_POINTER_OFFSET, X87_OPCODE_OFFSET, X87_SIZE,
            X87_STATUS_OFFSET, X87_TAG_OFFSET,
        },
    },
    cpu::x86::{Register32, X87State},
};

use super::locals::{CANCEL_PTR, EXIT_PTR, MEMORY_BYTES, STATE_PTR, register};

pub(super) fn preflight(code: &mut InstructionSink<'_>) {
    end(code);
    code.local_get(MEMORY_BYTES).i64_gt_u();
    fail_if(code, 1);
    for (pointer, size) in [
        (STATE_PTR, STATE_SIZE),
        (EXIT_PTR, EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        start(code);
        code.local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(size as i64)
            .i64_add()
            .i64_lt_u()
            .local_get(pointer)
            .i64_extend_i32_u();
        end(code);
        code.i64_lt_u().i32_and();
        fail_if(code, 1);
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3FP")),
        (
            4,
            u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, X87_SIZE as u32),
        (12, 0),
        (36, 0),
        (120, 0),
        (124, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(memarg(offset, 2))
            .i32_const(expected as i32)
            .i32_ne();
        fail_if(code, 2);
    }
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_OPCODE_OFFSET, 1))
        .i32_const(0x7ff)
        .i32_gt_u();
    fail_if(code, 2);
}

pub(super) fn initialize(code: &mut InstructionSink<'_>) {
    let state = X87State::default();
    for (offset, value) in [
        (
            X87_CONTROL_OFFSET,
            u32::from(state.control) | (u32::from(state.status) << 16),
        ),
        (
            X87_TAG_OFFSET,
            u32::from(state.tag) | (u32::from(state.opcode) << 16),
        ),
        (X87_INSTRUCTION_POINTER_OFFSET, state.instruction_pointer),
        (X87_DATA_POINTER_OFFSET, state.data_pointer),
        (
            X87_CODE_SELECTOR_OFFSET,
            u32::from(state.code_selector) | (u32::from(state.data_selector) << 16),
        ),
    ] {
        code.local_get(STATE_PTR)
            .i32_const(value as i32)
            .i32_store(memarg(offset, 2));
    }
}

pub(super) fn clear_exceptions(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_STATUS_OFFSET, 1))
        .i32_const(0x7f00)
        .i32_and()
        .i32_store16(memarg(X87_STATUS_OFFSET, 1));
}

pub(super) fn status_to_ax(code: &mut InstructionSink<'_>) {
    code.local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(STATE_PTR)
        .i32_load16_u(memarg(X87_STATUS_OFFSET, 1))
        .i32_or()
        .local_set(register(Register32::Eax));
}

fn start(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const(X87_OFFSET as i64)
        .i64_add();
}

fn end(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const((X87_OFFSET + X87_SIZE) as i64)
        .i64_add();
}

fn fail_if(code: &mut InstructionSink<'_>, status: i32) {
    code.if_(BlockType::Empty).i32_const(status).return_().end();
}

fn memarg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: (X87_OFFSET + offset) as u64,
        align,
        memory_index: 0,
    }
}
