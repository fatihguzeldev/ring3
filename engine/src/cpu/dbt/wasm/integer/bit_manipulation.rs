use crate::cpu::dbt::wasm::imports::Imports;
use wasm_encoder::{BlockType, InstructionSink};

use super::{ADDRESS, FLAGS, LHS, MEMORY_BYTES, RESULT, RHS, Register32, memory, register};
use crate::cpu::x86::{bits::BitManipulationKind, ir::Value32};

pub(super) fn bit_manipulation(
    code: &mut InstructionSink<'_>,
    kind: BitManipulationKind,
    destination: Register32,
    source: Value32,
    imports: Imports,
    exit_depth: u32,
) {
    // check the complete memory source before changing any guest register or flag.
    load_source(code, source, imports, exit_depth);
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
        BitManipulationKind::ZeroHigh { index } => zero_high(code, index),
        BitManipulationKind::Deposit { value } | BitManipulationKind::ExtractMasked { value } => {
            parallel_bits(
                code,
                value,
                matches!(kind, BitManipulationKind::Deposit { .. }),
            );
        }
        BitManipulationKind::ShiftLeft { count }
        | BitManipulationKind::ShiftLogicalRight { count }
        | BitManipulationKind::ShiftArithmeticRight { count } => {
            code.local_get(LHS).local_get(register(count));
            match kind {
                BitManipulationKind::ShiftLeft { .. } => {
                    code.i32_shl();
                }
                BitManipulationKind::ShiftLogicalRight { .. } => {
                    code.i32_shr_u();
                }
                BitManipulationKind::ShiftArithmeticRight { .. } => {
                    code.i32_shr_s();
                }
                _ => unreachable!(),
            }
            code.local_set(RESULT);
        }
        BitManipulationKind::RotateRight { count } => {
            code.local_get(LHS)
                .i32_const(i32::from(count))
                .i32_rotr()
                .local_set(RESULT);
        }
    }
    flags(code, kind);
    code.local_get(RESULT).local_set(register(destination));
}

pub(super) fn multiply_extended(
    code: &mut InstructionSink<'_>,
    high: Register32,
    low: Register32,
    source: Value32,
    imports: Imports,
    exit_depth: u32,
) {
    load_source(code, source, imports, exit_depth);
    // preflight has finished using this i64 local; retain both halves before either write.
    code.local_get(register(Register32::Edx))
        .i64_extend_i32_u()
        .local_get(RESULT)
        .i64_extend_i32_u()
        .i64_mul()
        .local_set(MEMORY_BYTES)
        .local_get(MEMORY_BYTES)
        .i32_wrap_i64()
        .local_set(register(low))
        .local_get(MEMORY_BYTES)
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(register(high));
}

fn load_source(code: &mut InstructionSink<'_>, source: Value32, imports: Imports, exit_depth: u32) {
    match source {
        Value32::Register(source) => {
            code.local_get(register(source)).local_set(RESULT);
        }
        Value32::Immediate(value) => {
            code.i32_const(value as i32).local_set(RESULT);
        }
        Value32::Memory(address) => memory::load_result(code, address, imports, exit_depth),
    }
}

fn zero_high(code: &mut InstructionSink<'_>, index: Register32) {
    code.local_get(register(index))
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
        .local_set(RESULT);
}

fn parallel_bits(code: &mut InstructionSink<'_>, value: Register32, deposit: bool) {
    code.local_get(LHS)
        .local_set(RHS)
        .local_get(register(value))
        .local_set(LHS)
        .i32_const(0)
        .local_set(RESULT);
    if !deposit {
        // the memory address is no longer needed after the complete source read.
        code.i32_const(1).local_set(ADDRESS);
    }
    code.block(BlockType::Empty)
        .loop_(BlockType::Empty)
        .local_get(RHS)
        .i32_eqz()
        .br_if(1)
        .local_get(LHS);
    if deposit {
        code.i32_const(1).i32_and();
    } else {
        lowest_mask_bit(code);
        code.i32_and();
    }
    code.if_(BlockType::Empty).local_get(RESULT);
    if deposit {
        lowest_mask_bit(code);
    } else {
        code.local_get(ADDRESS);
    }
    code.i32_or().local_set(RESULT).end();
    if deposit {
        code.local_get(LHS).i32_const(1).i32_shr_u().local_set(LHS);
    } else {
        code.local_get(ADDRESS)
            .i32_const(1)
            .i32_shl()
            .local_set(ADDRESS);
    }
    code.local_get(RHS)
        .local_get(RHS)
        .i32_const(1)
        .i32_sub()
        .i32_and()
        .local_set(RHS)
        .br(0)
        .end()
        .end();
}

fn lowest_mask_bit(code: &mut InstructionSink<'_>) {
    code.local_get(RHS)
        .i32_const(0)
        .local_get(RHS)
        .i32_sub()
        .i32_and();
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
    if matches!(
        kind,
        BitManipulationKind::Deposit { .. }
            | BitManipulationKind::ExtractMasked { .. }
            | BitManipulationKind::ShiftLeft { .. }
            | BitManipulationKind::ShiftLogicalRight { .. }
            | BitManipulationKind::ShiftArithmeticRight { .. }
            | BitManipulationKind::RotateRight { .. }
    ) {
        return;
    }
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
        BitManipulationKind::ZeroHigh { .. } => {
            code.local_get(RHS).i32_const(32).i32_ge_u().i32_or();
        }
        BitManipulationKind::AndNot { .. } | BitManipulationKind::Extract { .. } => {}
        _ => unreachable!(),
    }
    code.local_set(FLAGS);
}
