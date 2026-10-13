use wasm_encoder::{BlockType, InstructionSink, MemArg};

use super::super::locals::*;
use crate::abi::{
    ABI_VERSION, X86_INTEGER_PROFILE,
    arena::YMM_OFFSET,
    x86::{EXIT_SIZE, STATE_SIZE, YMM_SIZE},
};

pub(super) fn emit(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const((YMM_OFFSET + YMM_SIZE) as i64)
        .i64_add()
        .local_get(MEMORY_BYTES)
        .i64_gt_u()
        .if_(BlockType::Empty)
        .i32_const(1)
        .return_()
        .end();
    for (pointer, size) in [
        (STATE_PTR, STATE_SIZE),
        (EXIT_PTR, EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        code.local_get(STATE_PTR)
            .i64_extend_i32_u()
            .i64_const(YMM_OFFSET as i64)
            .i64_add()
            .local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(size as i64)
            .i64_add()
            .i64_lt_u()
            .local_get(pointer)
            .i64_extend_i32_u()
            .local_get(STATE_PTR)
            .i64_extend_i32_u()
            .i64_const((YMM_OFFSET + YMM_SIZE) as i64)
            .i64_add()
            .i64_lt_u()
            .i32_and()
            .if_(BlockType::Empty)
            .i32_const(1)
            .return_()
            .end();
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3YU")),
        (
            4,
            u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, YMM_SIZE as u32),
        (12, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(MemArg {
                offset: (YMM_OFFSET + offset) as u64,
                align: 2,
                memory_index: 0,
            })
            .i32_const(expected as i32)
            .i32_ne()
            .if_(BlockType::Empty)
            .i32_const(2)
            .return_()
            .end();
    }
}
