use wasm_encoder::InstructionSink;

use super::locals::*;
use crate::cpu::x86::decode::DecodedOperation;

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    vector: u8,
    exit_depth: u32,
) {
    let (reason, detail, completed) = match vector {
        // int imm8 is unconditional; into uses the separate of-dependent emitter.
        4 => (18, 0, true),
        // explicit windows application entries do not construct an idt frame or
        // pretend to complete the requested OS service.
        0x29 | 0x2e => (24, i32::from(vector), true),
        _ => (11, (i32::from(vector) << 3) | 2, false),
    };
    if completed {
        code.i32_const(instruction.next_pc().0 as i32)
            .local_set(EIP)
            .local_get(BUDGET)
            .i32_const(1)
            .i32_sub()
            .local_set(BUDGET)
            .local_get(RETIRED)
            .i32_const(1)
            .i32_add()
            .local_set(RETIRED);
    }
    code.i32_const(reason)
        .local_set(REASON)
        .i32_const(detail)
        .local_set(DETAIL);
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(exit_depth);
}
