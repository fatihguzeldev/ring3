use super::*;
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, X87State};
use std::mem::offset_of;

const PRECISION: u32 = 39;
const EXPONENT: u32 = 40;
const SIGN: u32 = 41;
const INEXACT: u32 = 42;
const INCREMENTED: u32 = 43;
const TEMPORARY: u32 = 45;
const X: u32 = 63;
const Y: u32 = 64;
const LOW: u32 = 65;
const HIGH: u32 = 66;
const CROSS_X: u32 = 67;
const CROSS_Y: u32 = 68;

struct Multiply {
    destination: u8,
    source: Option<u8>,
    pop: bool,
    address: Option<EffectiveAddress>,
    width: u32,
    waiting: bool,
    opcode: u16,
}

fn lower(operation: Operation) -> Option<Multiply> {
    let Operation::X87 {
        operation,
        address,
        waiting,
        opcode,
    } = operation
    else {
        return None;
    };
    let waiting = waiting || !operation.ignores_pending_exception();
    let (destination, source, pop, width) = match operation {
        X87Operation::Binary {
            kind: X87Binary::Multiply,
            destination,
            source,
            pop,
        } if destination < 8 && source < 8 && address.is_none() => {
            (destination, Some(source), pop, 0)
        }
        X87Operation::BinaryMemory {
            kind: X87Binary::Multiply,
            format: format @ (X87Format::Float32 | X87Format::Float64),
        } if address.is_some() => (
            0,
            None,
            false,
            if format == X87Format::Float64 { 8 } else { 4 },
        ),
        _ => return None,
    };
    Some(Multiply {
        destination,
        source,
        pop,
        address,
        width,
        waiting,
        opcode,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

fn halfword(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 1,
        memory_index: 0,
    }
}

fn memory_operand(out: &mut Output, address: EffectiveAddress, width: u32) {
    out.address(address);
    if width == 4 {
        out.read(4, INEXACT);
        out.c()
            .local_get(INEXACT)
            .i32_const(0x7f800000)
            .i32_and()
            .i32_const(0x7f800000)
            .i32_eq()
            .local_get(INEXACT)
            .i32_const(0x7fffffff)
            .i32_and()
            .i32_const(1)
            .i32_sub()
            .i32_const(0x7ffffe)
            .i32_le_u()
            .i32_or();
        x87::miss_if(out);
        x87::raw_from_f32_bits(out, INEXACT);
    } else {
        memory::read_words(out, &[INEXACT, INCREMENTED], 1);
        out.c()
            .local_get(INEXACT)
            .i64_extend_i32_u()
            .local_get(INCREMENTED)
            .i64_extend_i32_u()
            .i64_const(32)
            .i64_shl()
            .i64_or()
            .local_set(x87::RAW)
            .local_get(x87::RAW)
            .i64_const(0x7ff0_0000_0000_0000)
            .i64_and()
            .i64_const(0x7ff0_0000_0000_0000)
            .i64_eq()
            .local_get(x87::RAW)
            .i64_const(0x7fff_ffff_ffff_ffff)
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

fn product(out: &mut Output) {
    // four unsigned 32-bit products fit exactly in their 64-bit destinations.
    out.c()
        .local_get(X)
        .i64_const(0xffff_ffff)
        .i64_and()
        .local_get(Y)
        .i64_const(0xffff_ffff)
        .i64_and()
        .i64_mul()
        .local_set(LOW)
        .local_get(X)
        .i64_const(0xffff_ffff)
        .i64_and()
        .local_get(Y)
        .i64_const(32)
        .i64_shr_u()
        .i64_mul()
        .local_set(CROSS_X)
        .local_get(X)
        .i64_const(32)
        .i64_shr_u()
        .local_get(Y)
        .i64_const(0xffff_ffff)
        .i64_and()
        .i64_mul()
        .local_set(CROSS_Y)
        .local_get(X)
        .i64_const(32)
        .i64_shr_u()
        .local_get(Y)
        .i64_const(32)
        .i64_shr_u()
        .i64_mul()
        .local_get(CROSS_X)
        .i64_const(32)
        .i64_shr_u()
        .i64_add()
        .local_get(CROSS_Y)
        .i64_const(32)
        .i64_shr_u()
        .i64_add()
        .local_set(HIGH)
        .local_get(LOW)
        .local_set(TEMPORARY)
        .local_get(LOW)
        .local_get(CROSS_X)
        .i64_const(32)
        .i64_shl()
        .i64_add()
        .local_set(LOW)
        .local_get(HIGH)
        .local_get(LOW)
        .local_get(TEMPORARY)
        .i64_lt_u()
        .i64_extend_i32_u()
        .i64_add()
        .local_set(HIGH)
        .local_get(LOW)
        .local_set(TEMPORARY)
        .local_get(LOW)
        .local_get(CROSS_Y)
        .i64_const(32)
        .i64_shl()
        .i64_add()
        .local_set(LOW)
        .local_get(HIGH)
        .local_get(LOW)
        .local_get(TEMPORARY)
        .i64_lt_u()
        .i64_extend_i32_u()
        .i64_add()
        .local_set(HIGH);
}

fn reject_exponent(out: &mut Output) {
    out.c()
        .local_get(EXPONENT)
        .i32_const(1)
        .i32_sub()
        .i32_const(0x7ffd)
        .i32_gt_u();
    x87::miss_if(out);
}

fn round_product(out: &mut Output) {
    // normal input significands put the product's leading bit at 126 or 127.
    // discard k high-limb bits and the whole low limb, where k = 63 - p + topbit.
    out.c()
        .local_get(EXPONENT)
        .local_get(HIGH)
        .i64_const(63)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_add()
        .local_set(EXPONENT);
    reject_exponent(out);
    out.c()
        .i32_const(63)
        .local_get(PRECISION)
        .i32_sub()
        .i64_extend_i32_u()
        .local_get(HIGH)
        .i64_const(63)
        .i64_shr_u()
        .i64_add()
        .local_set(CROSS_X)
        .local_get(HIGH)
        .local_get(CROSS_X)
        .i64_shr_u()
        .local_set(X)
        .local_get(HIGH)
        .i64_const(1)
        .local_get(CROSS_X)
        .i64_shl()
        .i64_const(1)
        .i64_sub()
        .i64_and()
        .local_set(Y)
        .i64_const(1)
        .local_get(CROSS_X)
        .i64_const(1)
        .i64_sub()
        .i64_shl()
        .local_set(CROSS_Y)
        .local_get(Y)
        .local_get(LOW)
        .i64_or()
        .i64_eqz()
        .i32_eqz()
        .local_set(INEXACT)
        .local_get(Y)
        .local_get(CROSS_Y)
        .i64_gt_u()
        .local_get(Y)
        .local_get(CROSS_Y)
        .i64_eq()
        .local_get(LOW)
        .local_get(X)
        .i64_const(1)
        .i64_and()
        .i64_or()
        .i64_eqz()
        .i32_eqz()
        .i32_and()
        .i32_or()
        .local_set(INCREMENTED)
        .local_get(X)
        .local_get(INCREMENTED)
        .i64_extend_i32_u()
        .i64_add()
        .local_set(X)
        .local_get(X)
        .i64_const(1)
        .local_get(PRECISION)
        .i64_extend_i32_u()
        .i64_shl()
        .i64_eq()
        .if_(BlockType::Empty)
        .local_get(X)
        .i64_const(1)
        .i64_shr_u()
        .local_set(X)
        .local_get(EXPONENT)
        .i32_const(1)
        .i32_add()
        .local_set(EXPONENT)
        .end();
    reject_exponent(out);
    out.c()
        .local_get(X)
        .i32_const(64)
        .local_get(PRECISION)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_shl()
        .local_set(x87::RAW)
        .local_get(EXPONENT)
        .local_get(SIGN)
        .i32_or()
        .local_set(x87::TMP);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(Multiply {
        destination,
        source,
        pop,
        address,
        width,
        waiting,
        opcode,
    }) = lower(operation)
    else {
        return false;
    };
    x87::enter(out, waiting);
    out.c()
        .local_get(x87::STATE)
        .i32_load16_u(halfword(offset_of!(X87State, control) as u64))
        .i32_const(!0x40)
        .i32_and()
        .local_set(PRECISION)
        .local_get(PRECISION)
        .i32_const(0x003f)
        .i32_ne()
        .local_get(PRECISION)
        .i32_const(0x023f)
        .i32_ne()
        .i32_and();
    x87::miss_if(out);
    out.c()
        .local_get(PRECISION)
        .i32_const(0x023f)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(53)
        .else_()
        .i32_const(24)
        .end()
        .local_set(PRECISION);
    x87::read_raw(out, destination);
    out.c()
        .local_get(x87::RAW)
        .local_set(X)
        .local_get(x87::TMP)
        .i32_const(0x7fff)
        .i32_and()
        .local_set(EXPONENT)
        .local_get(x87::TMP)
        .i32_const(0x8000)
        .i32_and()
        .local_set(SIGN);
    if let Some(source) = source {
        x87::read_raw(out, source);
    } else {
        memory_operand(out, address.expect("decoded memory multiplication"), width);
    }
    out.c()
        .local_get(x87::RAW)
        .local_set(Y)
        .local_get(EXPONENT)
        .local_get(x87::TMP)
        .i32_const(0x7fff)
        .i32_and()
        .i32_add()
        .i32_const(16383)
        .i32_sub()
        .local_set(EXPONENT)
        .local_get(SIGN)
        .local_get(x87::TMP)
        .i32_const(0x8000)
        .i32_and()
        .i32_xor()
        .local_set(SIGN)
        .local_get(X)
        .i64_eqz()
        .local_get(Y)
        .i64_eqz()
        .i32_or()
        .if_(BlockType::Empty)
        .i64_const(0)
        .local_set(x87::RAW)
        .local_get(SIGN)
        .local_set(x87::TMP)
        .i32_const(0)
        .local_set(INEXACT)
        .i32_const(0)
        .local_set(INCREMENTED)
        .else_();
    product(out);
    round_product(out);
    out.c().end();
    // every domain and exponent guard precedes the first architectural write.
    x87::write_raw(out, destination);
    out.c()
        .local_get(x87::STATE)
        .local_get(x87::STATE)
        .i32_load16_u(halfword(offset_of!(X87State, status) as u64))
        .i32_const(!0x200)
        .i32_and()
        .local_get(INEXACT)
        .i32_const(5)
        .i32_shl()
        .i32_or()
        .local_get(INCREMENTED)
        .i32_const(9)
        .i32_shl()
        .i32_or()
        .i32_store16(halfword(offset_of!(X87State, status) as u64));
    if pop {
        x87::pop(out);
    }
    x87::metadata(out, opcode, address.is_some());
    true
}
