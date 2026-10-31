use super::*;
use crate::cpu::x86::{X87Binary, X87Format, X87Operation, X87State};
use std::mem::offset_of;

const MEMORY_BITS: u32 = 39;
const RESULT_BITS: u32 = 40;
const NARROW: u32 = 47;
const LEFT: u32 = 50;
const RIGHT: u32 = 51;
const WIDE: u32 = 52;
const RESULT: u32 = 53;
const VIRTUAL_RIGHT: u32 = 54;
const ERROR: u32 = 55;

struct Arithmetic {
    kind: X87Binary,
    destination: u8,
    source: Option<u8>,
    pop: bool,
    address: Option<EffectiveAddress>,
    waiting: bool,
    opcode: u16,
}

fn lower(operation: Operation) -> Option<Arithmetic> {
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
    let (kind, destination, source, pop) = match operation {
        X87Operation::Binary {
            kind,
            destination,
            source,
            pop,
        } if destination < 8 && source < 8 && address.is_none() => {
            (kind, destination, Some(source), pop)
        }
        X87Operation::BinaryMemory {
            kind,
            format: X87Format::Float32,
        } if address.is_some() => (kind, 0, None, false),
        _ => return None,
    };
    if !matches!(
        kind,
        X87Binary::Add | X87Binary::Multiply | X87Binary::Divide
    ) {
        return None;
    }
    Some(Arithmetic {
        kind,
        destination,
        source,
        pop,
        address,
        waiting,
        opcode,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
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
    x87::miss_if(out);
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(Arithmetic {
        kind,
        destination,
        source,
        pop,
        address,
        waiting,
        opcode,
    }) = lower(operation)
    else {
        return false;
    };
    x87::enter(out, waiting);
    out.c()
        .local_get(x87::STATE)
        .i32_load16_u(MemArg {
            offset: offset_of!(X87State, control) as u64,
            align: 1,
            memory_index: 0,
        })
        .i32_const(!0x40)
        .i32_and()
        .i32_const(0x003f)
        .i32_ne();
    x87::miss_if(out);
    x87::read_raw_f32(out, destination, LEFT);
    if let Some(source) = source {
        x87::read_raw_f32(out, source, RIGHT);
    } else {
        out.address(address.expect("decoded memory arithmetic"));
        out.read(4, MEMORY_BITS);
        reject_non_normal(out, MEMORY_BITS);
        out.c()
            .local_get(MEMORY_BITS)
            .f32_reinterpret_i32()
            .f64_promote_f32()
            .local_set(RIGHT);
    }
    if kind == X87Binary::Divide {
        out.c().local_get(RIGHT).f64_const(0.0.into()).f64_eq();
        x87::miss_if(out);
    }
    out.c().local_get(LEFT).local_get(RIGHT);
    match kind {
        X87Binary::Add => {
            out.c().f64_add();
        }
        X87Binary::Multiply => {
            out.c().f64_mul();
        }
        X87Binary::Divide => {
            out.c().f64_div();
        }
        _ => unreachable!(),
    }
    out.c().local_set(WIDE);
    if kind == X87Binary::Add {
        // error-free TwoSum also detects addends lost by binary64 itself.
        // binary32 inputs cannot overflow or underflow any binary64 intermediate.
        out.c()
            .local_get(WIDE)
            .local_get(LEFT)
            .f64_sub()
            .local_set(VIRTUAL_RIGHT)
            .local_get(LEFT)
            .local_get(WIDE)
            .local_get(VIRTUAL_RIGHT)
            .f64_sub()
            .f64_sub()
            .local_get(RIGHT)
            .local_get(VIRTUAL_RIGHT)
            .f64_sub()
            .f64_add()
            .local_set(ERROR)
            .local_get(ERROR)
            .f64_const(0.0.into())
            .f64_ne();
        x87::miss_if(out);
    }
    out.c()
        .local_get(WIDE)
        .f32_demote_f64()
        .local_tee(NARROW)
        .f64_promote_f32()
        .local_set(RESULT)
        .local_get(NARROW)
        .i32_reinterpret_f32()
        .local_set(RESULT_BITS);
    reject_non_normal(out, RESULT_BITS);
    if kind == X87Binary::Divide {
        // RESULT and RIGHT are binary32 values: their product is exact in f64.
        // equality proves exact division without trusting a rounded f64 quotient.
        out.c()
            .local_get(RESULT)
            .local_get(RIGHT)
            .f64_mul()
            .local_get(LEFT)
            .f64_ne();
    } else {
        // multiplication of two binary32 operands has at most48 significant bits.
        out.c().local_get(RESULT).local_get(WIDE).f64_ne();
    }
    x87::miss_if(out);
    // only exact results commit here: PE stays sticky and the new C1 is clear.
    x87::commit_raw_f32(out, destination, RESULT);
    x87::clear_c1(out);
    if pop {
        x87::pop(out);
    }
    x87::metadata(out, opcode, address.is_some());
    true
}
