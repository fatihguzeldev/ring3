use wasm_encoder::{BlockType, InstructionSink};

use super::{FLAGS, LHS, RESULT, RHS, Register32, memory, register};
use crate::cpu::x86::{bits::BitManipulationKind, ir::Value32};

pub(super) fn bit_manipulation(
    code: &mut InstructionSink<'_>,
    kind: BitManipulationKind,
    destination: Register32,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    // check the complete memory source before changing any guest register or flag.
    match source {
        Value32::Register(source) => {
            code.local_get(register(source)).local_set(RESULT);
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32).local_set(RESULT);
        }
        Value32::Memory(address) => memory::load_result(code, address, imports, exit_depth),
    }
    code.local_get(RESULT).local_set(LHS);
    match kind {
        BitManipulationKind::AndNot { left } => {
            code.local_get(register(left))
                .i32_const(-1)
                .i32_xor()
                .local_get(LHS)
                .i32_and()
                .local_set(RESULT);
        }
        BitManipulationKind::Extract { control } => extract(code, control),
        BitManipulationKind::IsolateLowest => {
            code.i32_const(0)
                .local_get(LHS)
                .i32_sub()
                .local_get(LHS)
                .i32_and()
                .local_set(RESULT);
        }
        BitManipulationKind::MaskLowest | BitManipulationKind::ResetLowest => {
            code.local_get(LHS).i32_const(1).i32_sub().local_get(LHS);
            if kind == BitManipulationKind::MaskLowest {
                code.i32_xor();
            } else {
                code.i32_and();
            }
            code.local_set(RESULT);
        }
    }
    flags(code, kind);
    code.local_get(RESULT).local_set(register(destination));
}

fn extract(code: &mut InstructionSink<'_>, control: Register32) {
    code.local_get(register(control))
        .local_set(RHS)
        .local_get(RHS)
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT)
        .local_get(RESULT)
        .i32_const(32)
        .i32_ge_u()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(0)
        .else_()
        .local_get(LHS)
        .local_get(RESULT)
        .i32_shr_u()
        .local_set(LHS)
        .local_get(RHS)
        .i32_const(8)
        .i32_shr_u()
        .i32_const(0xff)
        .i32_and()
        .local_set(RHS)
        .local_get(RHS)
        .i32_const(32)
        .i32_ge_u()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(LHS)
        .else_()
        .local_get(LHS)
        .i32_const(1)
        .local_get(RHS)
        .i32_shl()
        .i32_const(1)
        .i32_sub()
        .i32_and()
        .end()
        .end()
        .local_set(RESULT);
}

fn flags(code: &mut InstructionSink<'_>, kind: BitManipulationKind) {
    // AF/PF are undefined; BEXTR also leaves SF undefined. clear these consistently.
    code.local_get(FLAGS).i32_const(!0x8d5).i32_and();
    if kind != BitManipulationKind::MaskLowest {
        code.local_get(RESULT)
            .i32_eqz()
            .i32_const(6)
            .i32_shl()
            .i32_or();
    }
    if !matches!(kind, BitManipulationKind::Extract { .. }) {
        code.local_get(RESULT)
            .i32_const(31)
            .i32_shr_u()
            .i32_const(7)
            .i32_shl()
            .i32_or();
    }
    match kind {
        BitManipulationKind::IsolateLowest => {
            code.local_get(LHS).i32_eqz().i32_eqz().i32_or();
        }
        BitManipulationKind::MaskLowest | BitManipulationKind::ResetLowest => {
            code.local_get(LHS).i32_eqz().i32_or();
        }
        BitManipulationKind::AndNot { .. } | BitManipulationKind::Extract { .. } => {}
    }
    code.local_set(FLAGS);
}
