use super::*;
use crate::cpu::x86::simd::{
    FloatKind, SimdKind, SimdOperand, SimdOperation, SimdRegister, SimdState,
};
use std::mem::{offset_of, size_of};

const STATE: u32 = 58;
const WORDS: [u32; 4] = [59, 60, 61, 62];
const CHECKED_STATE: u32 = 70;
// each instruction owns these floating scratch locals until it returns. no
// x87 instruction or canonical service runs during a speculative SIMD lane.
const NARROW: u32 = 47;
const LEFT: u32 = 50;
const RIGHT: u32 = 51;
const WIDE: u32 = 52;
const RESULT: u32 = 53;

const MXCSR: u64 = offset_of!(SimdState, mxcsr) as u64;
const XMM: u64 = offset_of!(SimdState, xmm) as u64;

const _: () = {
    assert!(size_of::<SimdState>() == 132);
    assert!(MXCSR == 0 && XMM == 4);
};

fn xmm(operand: SimdOperand) -> bool {
    matches!(operand, SimdOperand::Register(SimdRegister::Xmm(index)) if index < 8)
}

fn supported(operation: SimdOperation) -> bool {
    if operation.vector_bytes != 16
        || operation.scalar
        || operation.immediate.is_some()
        || operation.implicit_address.is_some()
        || operation.memory_bytes
            != if operation.reads_memory() || operation.writes_memory() {
                16
            } else {
                0
            }
    {
        return false;
    }
    let source = xmm(operation.source) || matches!(operation.source, SimdOperand::Memory(_));
    match operation.kind {
        SimdKind::Move if operation.lane_bytes == 16 => {
            xmm(operation.destination) && source
                || matches!(operation.destination, SimdOperand::Memory(_)) && xmm(operation.source)
        }
        SimdKind::Float(FloatKind::Add) if operation.lane_bytes == 4 => {
            xmm(operation.destination) && source
        }
        _ => false,
    }
}

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(*operation, Operation::Simd(operation) if supported(operation))
}

fn miss_if(out: &mut Output) {
    out.c().if_(BlockType::Empty);
    out.exit(7);
    out.c().end();
}

fn pointer(out: &mut Output, local: u32, length: i64) {
    out.c()
        .local_get(local)
        .i32_eqz()
        .local_get(local)
        .i32_const(3)
        .i32_and()
        .i32_or()
        .local_get(local)
        .i64_extend_i32_u()
        .i64_const(length)
        .i64_add()
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .i64_gt_u()
        .i32_or();
    miss_if(out);
}

fn enter(out: &mut Output) {
    // an x87 proof covers only the 36-byte prefix. validate this bank's
    // 40-byte view independently, once in each function invocation.
    out.c()
        .local_get(CHECKED_STATE)
        .i32_eqz()
        .if_(BlockType::Empty);
    out.c().local_get(0).i32_load(mem(124)).local_set(A);
    pointer(out, A, 40);
    out.c()
        .local_get(A)
        .i32_load(mem(0))
        .i32_const(1)
        .i32_ne()
        .local_get(A)
        .i32_load(mem(4))
        .i32_const(40)
        .i32_lt_u()
        .i32_or();
    miss_if(out);
    out.c().local_get(A).i32_load(mem(36)).local_set(STATE);
    pointer(out, STATE, size_of::<SimdState>() as i64);
    out.c()
        .local_get(STATE)
        .local_set(CHECKED_STATE)
        .local_get(A)
        .local_set(x87::CHECKED_VIEW)
        .end()
        .local_get(CHECKED_STATE)
        .local_set(STATE);
    // SSE has no pending-x87 exception check and does not alias its stack.
    out.c()
        .local_get(STATE)
        .i32_load(mem(MXCSR))
        .i32_const(!0xffff)
        .i32_and();
    miss_if(out);
}

fn cell(index: u8, lane: usize) -> u64 {
    XMM + u64::from(index) * 16 + lane as u64 * 4
}

fn source(out: &mut Output, operand: SimdOperand, alignment: u32) {
    match operand {
        SimdOperand::Register(SimdRegister::Xmm(index)) => {
            for (lane, local) in WORDS.into_iter().enumerate() {
                out.c()
                    .local_get(STATE)
                    .i32_load(mem(cell(index, lane)))
                    .local_set(local);
            }
        }
        SimdOperand::Memory(address) => {
            out.address(address);
            memory::read_words(out, &WORDS, alignment);
        }
        _ => unreachable!("supported SIMD source"),
    }
}

fn commit(out: &mut Output, operand: SimdOperand, alignment: u32) {
    match operand {
        SimdOperand::Register(SimdRegister::Xmm(index)) => {
            for (lane, local) in WORDS.into_iter().enumerate() {
                out.c()
                    .local_get(STATE)
                    .local_get(local)
                    .i32_store(mem(cell(index, lane)));
            }
        }
        SimdOperand::Memory(address) => {
            out.address(address);
            memory::write_words(out, &WORDS, alignment);
        }
        _ => unreachable!("supported SIMD destination"),
    }
}

fn reject_non_normal(out: &mut Output, bits: u32) {
    out.c()
        .local_get(bits)
        .i32_const(0x7f800000)
        .i32_and()
        .i32_const(0x7f800000)
        .i32_eq()
        .local_get(bits)
        .i32_const(0x7fffffff)
        .i32_and()
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffffe)
        .i32_le_u()
        .i32_or();
    miss_if(out);
}

fn add_lane(out: &mut Output, destination: u8, lane: usize, bits: u32) {
    out.c()
        .local_get(STATE)
        .i32_load(mem(cell(destination, lane)))
        .local_set(A);
    reject_non_normal(out, A);
    reject_non_normal(out, bits);
    out.c()
        .local_get(A)
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_set(LEFT)
        .local_get(bits)
        .f32_reinterpret_i32()
        .f64_promote_f32()
        .local_set(RIGHT)
        .local_get(LEFT)
        .local_get(RIGHT)
        .f64_add()
        .local_set(WIDE);
    // an inexact f64 sum of binary32 inputs requires a wide exponent gap.
    // subtracting the larger operand is then exact by Sterbenz and exposes
    // the lost addend. check both orders to avoid selecting that operand.
    // the 2^-149 input grid keeps every nonzero exact difference normal in f64.
    out.c()
        .local_get(WIDE)
        .local_get(LEFT)
        .f64_sub()
        .local_get(RIGHT)
        .f64_ne()
        .local_get(WIDE)
        .local_get(RIGHT)
        .f64_sub()
        .local_get(LEFT)
        .f64_ne()
        .i32_or();
    miss_if(out);
    out.c()
        .local_get(WIDE)
        .f32_demote_f64()
        .local_tee(NARROW)
        .f64_promote_f32()
        .local_set(RESULT)
        .local_get(NARROW)
        .i32_reinterpret_f32()
        .local_set(bits);
    reject_non_normal(out, bits);
    out.c().local_get(RESULT).local_get(WIDE).f64_ne();
    miss_if(out);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Operation::Simd(operation) = operation else {
        return false;
    };
    if !supported(operation) {
        return false;
    }
    enter(out);
    if operation.kind == SimdKind::Float(FloatKind::Add) {
        // nearest, masked, DAZ/FTZ off; all sticky exception flags are retained.
        // exact admission never changes MXCSR and cannot synthesize a #XM.
        out.c()
            .local_get(STATE)
            .i32_load(mem(MXCSR))
            .i32_const(!0x3f)
            .i32_and()
            .i32_const(0x1f80)
            .i32_ne();
        miss_if(out);
    }
    let alignment = if operation.aligned { 16 } else { 1 };
    source(out, operation.source, alignment);
    if operation.kind == SimdKind::Float(FloatKind::Add) {
        let SimdOperand::Register(SimdRegister::Xmm(destination)) = operation.destination else {
            unreachable!("supported ADDPS destination")
        };
        for (lane, bits) in WORDS.into_iter().enumerate() {
            add_lane(out, destination, lane, bits);
        }
    }
    // no lane, MXCSR, x87 state or destination memory has changed before here.
    commit(out, operation.destination, alignment);
    true
}
