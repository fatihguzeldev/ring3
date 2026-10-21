use super::super::{CTX, Output, mem};
use super::{BANK_POINTER, SIMD_MXCSR, SIMD_XMM, memory};
use crate::cpu::x86::{
    ir::Operation,
    simd::{FloatKind, SimdKind, SimdOperand, SimdOperation, SimdRegister},
};
use wasm_encoder::BlockType;

const BANK: u32 = 37;
const SOURCE: u32 = 52;
const DESTINATION: u32 = 53;
const MXCSR: u32 = 58;
const EXCEPTIONS: u32 = 59;
const LEFT: u32 = 60;
const RIGHT: u32 = 61;
const RESULT: u32 = 62;
const LOW_SUM: u32 = 63;
const HIGH_SUM: u32 = 64;

fn xmm(operand: SimdOperand) -> Option<u8> {
    match operand {
        SimdOperand::Register(SimdRegister::Xmm(index)) if index < 8 => Some(index),
        _ => None,
    }
}

fn supported(op: SimdOperation) -> bool {
    if op.vector_bytes != 16 || op.scalar || op.immediate.is_some() || op.implicit_address.is_some()
    {
        return false;
    }
    let memory = op.reads_memory() || op.writes_memory();
    if memory && op.memory_bytes != 16 {
        return false;
    }
    match op.kind {
        SimdKind::Move if op.lane_bytes == 16 => {
            xmm(op.destination).is_some()
                && (xmm(op.source).is_some() || matches!(op.source, SimdOperand::Memory(_)))
                || xmm(op.source).is_some() && matches!(op.destination, SimdOperand::Memory(_))
        }
        SimdKind::Float(FloatKind::Add) if op.lane_bytes == 4 => {
            xmm(op.destination).is_some()
                && (xmm(op.source).is_some() || matches!(op.source, SimdOperand::Memory(_)))
        }
        _ => false,
    }
}

pub(super) fn begin(out: &mut Output<'_>, operation: Operation, next: u32) -> bool {
    let Operation::Simd(op) = operation else {
        return false;
    };
    if !supported(op) {
        return false;
    }
    out.c()
        .block(BlockType::Empty)
        .block(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(BANK_POINTER, 2))
        .local_tee(BANK)
        .i32_eqz()
        .br_if(0)
        .local_get(BANK)
        .i32_load(mem(SIMD_MXCSR, 2))
        .local_tee(MXCSR)
        .i32_const(!0xffff)
        .i32_and()
        .br_if(0);
    operand(out, op.source, op.aligned, false, SOURCE);
    operand(out, op.destination, op.aligned, true, DESTINATION);
    if op.kind == SimdKind::Move {
        out.c()
            .local_get(DESTINATION)
            .local_get(SOURCE)
            .v128_load(mem(0, 0))
            .v128_store(mem(0, 0));
        if op.writes_memory() {
            memory::commit(out);
        }
    } else {
        add(out);
    }
    out.set_pc(next);
    out.c().br(1).end();
    true
}

fn operand(out: &mut Output<'_>, operand: SimdOperand, aligned: bool, write: bool, pointer: u32) {
    match operand {
        SimdOperand::Register(SimdRegister::Xmm(index)) => {
            out.c()
                .local_get(BANK)
                .i32_const((SIMD_XMM + u64::from(index) * 16) as i32)
                .i32_add()
                .local_set(pointer);
        }
        SimdOperand::Memory(address) => memory::prepare(out, address, 16, write, aligned, pointer),
        _ => unreachable!(),
    }
}

fn normal_or_zero(out: &mut Output<'_>, local: u32) {
    out.c()
        .local_get(local)
        .i32_const(0x7fff_ffff)
        .i32x4_splat()
        .v128_and()
        .i32_const(0)
        .i32x4_splat()
        .i32x4_eq()
        .local_get(local)
        .i32_const(0x7fff_ffff)
        .i32x4_splat()
        .v128_and()
        .i32_const(0x0080_0000)
        .i32x4_splat()
        .i32x4_sub()
        .i32_const(0x7eff_ffff)
        .i32x4_splat()
        .i32x4_le_u()
        .v128_or()
        .i32x4_all_true()
        .i32_eqz()
        .br_if(0);
}

fn high_pair(out: &mut Output<'_>, local: u32) {
    out.c()
        .local_get(local)
        .local_get(local)
        .i8x16_shuffle([8, 9, 10, 11, 12, 13, 14, 15, 8, 9, 10, 11, 12, 13, 14, 15])
        .f64x2_promote_low_f32x4();
}

fn add(out: &mut Output<'_>) {
    out.c()
        .local_get(MXCSR)
        .i32_const(!0x3f)
        .i32_and()
        .i32_const(0x1f80)
        .i32_ne()
        .br_if(0)
        .local_get(DESTINATION)
        .v128_load(mem(0, 0))
        .local_set(LEFT)
        .local_get(SOURCE)
        .v128_load(mem(0, 0))
        .local_set(RIGHT);
    normal_or_zero(out, LEFT);
    normal_or_zero(out, RIGHT);
    // within this exponent gap binary64 preserves the exact binary32 sum.
    out.c()
        .local_get(LEFT)
        .i32_const(23)
        .i32x4_shr_u()
        .i32_const(255)
        .i32x4_splat()
        .v128_and()
        .local_get(RIGHT)
        .i32_const(23)
        .i32x4_shr_u()
        .i32_const(255)
        .i32x4_splat()
        .v128_and()
        .i32x4_sub()
        .i32_const(29)
        .i32x4_splat()
        .i32x4_add()
        .i32_const(58)
        .i32x4_splat()
        .i32x4_le_u()
        .local_get(LEFT)
        .i32_const(0x7fff_ffff)
        .i32x4_splat()
        .v128_and()
        .i32_const(0)
        .i32x4_splat()
        .i32x4_eq()
        .v128_or()
        .local_get(RIGHT)
        .i32_const(0x7fff_ffff)
        .i32x4_splat()
        .v128_and()
        .i32_const(0)
        .i32x4_splat()
        .i32x4_eq()
        .v128_or()
        .i32x4_all_true()
        .i32_eqz()
        .br_if(0)
        .local_get(LEFT)
        .local_get(RIGHT)
        .f32x4_add()
        .local_set(RESULT);
    normal_or_zero(out, RESULT);
    // a nonzero sum of finite binary32 values cannot lie below the smallest subnormal.
    out.c()
        .local_get(LEFT)
        .f64x2_promote_low_f32x4()
        .local_get(RIGHT)
        .f64x2_promote_low_f32x4()
        .f64x2_add()
        .local_set(LOW_SUM);
    high_pair(out, LEFT);
    high_pair(out, RIGHT);
    out.c()
        .f64x2_add()
        .local_set(HIGH_SUM)
        .local_get(LOW_SUM)
        .local_get(RESULT)
        .f64x2_promote_low_f32x4()
        .f64x2_ne()
        .v128_any_true()
        .local_set(EXCEPTIONS)
        .local_get(HIGH_SUM);
    high_pair(out, RESULT);
    out.c()
        .f64x2_ne()
        .v128_any_true()
        .local_get(EXCEPTIONS)
        .i32_or()
        .i32_const(5)
        .i32_shl()
        .local_set(EXCEPTIONS)
        .local_get(DESTINATION)
        .local_get(RESULT)
        .v128_store(mem(0, 0))
        .local_get(BANK)
        .local_get(MXCSR)
        .local_get(EXCEPTIONS)
        .i32_or()
        .i32_store(mem(SIMD_MXCSR, 2));
}
