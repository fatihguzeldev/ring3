use super::*;
use crate::cpu::x86::{X87Format, X87Operation, X87State};
use std::mem::offset_of;

const LEFT: u32 = 63;
const RIGHT: u32 = 64;
const LEFT_SIGN_EXP: u32 = 39;
const RIGHT_SIGN_EXP: u32 = 40;
const ORDER: u32 = 41;
const STATUS: u64 = offset_of!(X87State, status) as u64;

#[derive(Clone, Copy)]
enum Operand {
    Register(u8),
    Memory(EffectiveAddress, u32),
}

struct Comparison {
    operand: Operand,
    pop: u8,
    waiting: bool,
    opcode: u16,
}

fn lower(operation: Operation) -> Option<Comparison> {
    let Operation::X87 {
        operation,
        address,
        waiting,
        opcode,
    } = operation
    else {
        return None;
    };
    let (operand, pop) = match operation {
        X87Operation::Compare {
            index,
            pop,
            integer_flags: false,
            ..
        } if index < 8 && pop <= 2 && address.is_none() => (Operand::Register(index), pop),
        X87Operation::CompareMemory {
            format: X87Format::Float32,
            pop,
        } => (Operand::Memory(address?, 4), u8::from(pop)),
        X87Operation::CompareMemory {
            format: X87Format::Float64,
            pop,
        } => (Operand::Memory(address?, 8), u8::from(pop)),
        _ => return None,
    };
    Some(Comparison {
        operand,
        pop,
        waiting: waiting || !operation.ignores_pending_exception(),
        opcode,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

fn fp_mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 1,
        memory_index: 0,
    }
}

fn save(out: &mut Output, significand: u32, sign_exp: u32) {
    out.c()
        .local_get(x87::RAW)
        .local_set(significand)
        .local_get(x87::TMP)
        .local_set(sign_exp);
}

fn memory_operand(out: &mut Output, address: EffectiveAddress, width: u32) {
    out.address(address);
    if width == 4 {
        out.read(4, R);
        // zero and finite normal binary32 are exact raw80 operands.
        out.c()
            .local_get(R)
            .i32_const(0x7f800000)
            .i32_and()
            .i32_const(0x7f800000)
            .i32_eq()
            .local_get(R)
            .i32_const(0x7fffffff)
            .i32_and()
            .i32_const(1)
            .i32_sub()
            .i32_const(0x007ffffe)
            .i32_le_u()
            .i32_or();
        x87::miss_if(out);
        x87::raw_from_f32_bits(out, R);
    } else {
        memory::read_words(out, &[A, B], 1);
        out.c()
            .local_get(A)
            .i64_extend_i32_u()
            .local_get(B)
            .i64_extend_i32_u()
            .i64_const(32)
            .i64_shl()
            .i64_or()
            .local_set(x87::RAW)
            .local_get(x87::RAW)
            .i64_const(52)
            .i64_shr_u()
            .i64_const(0x7ff)
            .i64_and()
            .i64_const(0x7ff)
            .i64_eq()
            .local_get(x87::RAW)
            .i64_const(i64::MAX)
            .i64_and()
            .i64_const(1)
            .i64_sub()
            .i64_const(0x000f_ffff_ffff_fffe)
            .i64_le_u()
            .i32_or();
        x87::miss_if(out);
        x87::raw_from_f64_bits(out);
    }
}

fn exponent(out: &mut Output, local: u32) {
    out.c().local_get(local).i32_const(0x7fff).i32_and();
}

fn same_magnitude(out: &mut Output) {
    exponent(out, LEFT_SIGN_EXP);
    exponent(out, RIGHT_SIGN_EXP);
    out.c()
        .i32_eq()
        .local_get(LEFT)
        .local_get(RIGHT)
        .i64_eq()
        .i32_and();
}

fn less_magnitude(out: &mut Output) {
    exponent(out, LEFT_SIGN_EXP);
    exponent(out, RIGHT_SIGN_EXP);
    out.c().i32_lt_u();
    exponent(out, LEFT_SIGN_EXP);
    exponent(out, RIGHT_SIGN_EXP);
    out.c()
        .i32_eq()
        .local_get(LEFT)
        .local_get(RIGHT)
        .i64_lt_u()
        .i32_and()
        .i32_or();
}

fn order(out: &mut Output) {
    // signs of zero do not participate in ordering. all other admitted raw80
    // operands have a normal exponent and explicit integer significand bit.
    out.c()
        .local_get(LEFT)
        .i64_eqz()
        .local_get(RIGHT)
        .i64_eqz()
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x4000)
        .else_()
        .local_get(LEFT_SIGN_EXP)
        .local_get(RIGHT_SIGN_EXP)
        .i32_xor()
        .i32_const(0x8000)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .local_get(LEFT_SIGN_EXP)
        .i32_const(0x8000)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x100)
        .else_()
        .i32_const(0)
        .end()
        .else_();
    same_magnitude(out);
    out.c()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x4000)
        .else_();
    less_magnitude(out);
    out.c()
        .local_get(LEFT_SIGN_EXP)
        .i32_const(15)
        .i32_shr_u()
        .i32_xor()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(0x100)
        .else_()
        .i32_const(0)
        .end()
        .end()
        .end()
        .end()
        .local_set(ORDER);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(comparison) = lower(operation) else {
        return false;
    };
    x87::enter(out, comparison.waiting);
    match comparison.operand {
        Operand::Memory(address, width) => {
            // the canonical owner reads the full operand before inspecting ST0.
            // every miss remains effect-free and leaves exact special handling
            // and any later fault to the same own instruction service.
            memory_operand(out, address, width);
            save(out, RIGHT, RIGHT_SIGN_EXP);
            x87::read_raw(out, 0);
            save(out, LEFT, LEFT_SIGN_EXP);
        }
        Operand::Register(index) => {
            x87::read_raw(out, 0);
            save(out, LEFT, LEFT_SIGN_EXP);
            x87::read_raw(out, index);
            save(out, RIGHT, RIGHT_SIGN_EXP);
        }
    }
    order(out);
    out.c()
        .local_get(x87::STATE)
        .local_get(x87::STATE)
        .i32_load16_u(fp_mem(STATUS))
        .i32_const(!0x4700)
        .i32_and()
        .local_get(ORDER)
        .i32_or()
        .i32_store16(fp_mem(STATUS));
    for _ in 0..comparison.pop {
        x87::pop(out);
    }
    x87::metadata(
        out,
        comparison.opcode,
        matches!(comparison.operand, Operand::Memory(..)),
    );
    true
}
