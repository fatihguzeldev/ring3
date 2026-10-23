use super::super::{CTX, LOGICAL_ADDRESS, Output, mem};
use crate::cpu::x86::ir::EffectiveAddress;
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const VALUES: u32 = 76;
const ACTIVE: u32 = 84;
const VALID: u32 = 85;
const DIRTY: u32 = 86;
pub(super) const CONTROL: u32 = 87;
const STATUS: u32 = 88;
const TAG: u32 = 89;
const TOP: u32 = 90;
const FIP: u32 = 91;
const FDP: u32 = 92;
const OPCODE: u32 = 93;
const CS: u32 = 94;
const DS: u32 = 95;
const BANK: u32 = 96;
const ROTATE: u32 = 97;
const EXPONENT: u32 = 44;
const PACKED: u32 = 47;

// valid slots hold exact architectural values; dirty slots also own the raw80 payload.
// logical rotation keeps that ownership coherent at every cfg join, including empty slots.
pub(super) fn enter(out: &mut Output<'_>) {
    out.c()
        .local_get(ACTIVE)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(super::BANK_POINTER, 2))
        .local_tee(BANK)
        .i32_eqz()
        .br_if(1)
        .local_get(BANK)
        .i32_load16_u(mem(0, 1))
        .local_tee(CONTROL)
        .i32_const(63)
        .i32_and()
        .i32_const(63)
        .i32_ne()
        .br_if(1);
    for (offset, local, wide) in [
        (2, STATUS, false),
        (4, TAG, false),
        (6, OPCODE, false),
        (8, FIP, true),
        (12, FDP, true),
        (16, CS, false),
        (18, DS, false),
    ] {
        out.c().local_get(BANK);
        if wide {
            out.c().i32_load(mem(offset, 2));
        } else {
            out.c().i32_load16_u(mem(offset, 1));
        }
        out.c().local_set(local);
    }
    out.c()
        .local_get(STATUS)
        .i32_const(11)
        .i32_shr_u()
        .i32_const(7)
        .i32_and()
        .local_set(TOP)
        .i32_const(0)
        .local_set(VALID)
        .i32_const(0)
        .local_set(DIRTY)
        .i32_const(0)
        .local_set(super::double::PENDING)
        .i32_const(1)
        .local_set(ACTIVE)
        .end();
    for (from, to) in [(BANK, 37), (STATUS, 41), (TAG, 42), (TOP, 38)] {
        out.c().local_get(from).local_set(to);
    }
}

pub(super) fn status(out: &mut Output<'_>) {
    resolve_status(out);
    out.c()
        .local_get(ACTIVE)
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(STATUS)
        .else_()
        .local_get(37)
        .i32_load16_u(mem(2, 1))
        .end()
        .local_set(41);
}

pub(super) fn clear_status_recipe(out: &mut Output<'_>) {
    out.c().i32_const(0).local_set(super::double::PENDING);
}

pub(in crate::cpu::core::emitter) fn resolve_status(out: &mut Output<'_>) {
    out.c()
        .local_get(ACTIVE)
        .local_get(super::double::PENDING)
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .if_(BlockType::Empty);
    for local in [
        super::double::PENDING,
        STATUS,
        super::double::PENDING_LEFT,
        super::double::PENDING_RIGHT,
        super::double::PENDING_RESULT,
    ] {
        out.c().local_get(local);
    }
    let resolver = out.internal(8);
    out.c().call(resolver).local_tee(STATUS).local_set(41);
    clear_status_recipe(out);
    out.c().end();
}

pub(in crate::cpu::core::emitter) fn emit_status_resolver() -> Function {
    super::double::emit_resolver()
}

pub(super) fn commit(out: &mut Output<'_>, opcode: u16, address: Option<EffectiveAddress>) {
    for (from, to) in [(41, STATUS), (42, TAG), (38, TOP)] {
        out.c().local_get(from).local_set(to);
    }
    let pc = out.pc;
    out.c()
        .i32_const(pc as i32)
        .local_set(FIP)
        .i32_const(i32::from(opcode))
        .local_set(OPCODE)
        .local_get(BANK)
        .i32_load16_u(mem(102, 1))
        .local_set(CS);
    if let Some(address) = address {
        out.c()
            .local_get(LOGICAL_ADDRESS)
            .local_set(FDP)
            .local_get(BANK)
            .i32_load16_u(mem(100 + address.resolved_segment().index() as u64 * 2, 1))
            .local_set(DS);
    }
}

pub(super) fn valid(out: &mut Output<'_>, index: u8) {
    assert!(index < 8);
    out.c().local_get(VALID).i32_const(1 << index).i32_and();
}

pub(super) fn load(out: &mut Output<'_>, index: u8, destination: u32) {
    out.c()
        .local_get(VALUES + u32::from(index))
        .local_set(destination);
}

pub(super) fn remember(out: &mut Output<'_>, index: u8, source: u32) {
    out.c()
        .local_get(source)
        .local_set(VALUES + u32::from(index))
        .local_get(VALID)
        .i32_const(1 << index)
        .i32_or()
        .local_set(VALID);
}

pub(super) fn write(out: &mut Output<'_>, index: u8, source: u32) {
    remember(out, index, source);
    out.c()
        .local_get(DIRTY)
        .i32_const(1 << index)
        .i32_or()
        .local_set(DIRTY);
}

pub(super) fn invalidate_slot(out: &mut Output<'_>, index: u8) {
    for local in [VALID, DIRTY] {
        out.c()
            .local_get(local)
            .i32_const(!(1 << index))
            .i32_and()
            .local_set(local);
    }
}

pub(super) fn push(out: &mut Output<'_>) {
    out.c().local_get(VALUES + 7).local_set(ROTATE);
    for index in (1..8).rev() {
        out.c()
            .local_get(VALUES + index - 1)
            .local_set(VALUES + index);
    }
    out.c().local_get(ROTATE).local_set(VALUES);
    for local in [VALID, DIRTY] {
        out.c()
            .local_get(local)
            .i32_const(1)
            .i32_shl()
            .local_get(local)
            .i32_const(7)
            .i32_shr_u()
            .i32_or()
            .i32_const(255)
            .i32_and()
            .local_set(local);
    }
}

pub(super) fn pop(out: &mut Output<'_>) {
    out.c().local_get(VALUES).local_set(ROTATE);
    for index in 0..7 {
        out.c()
            .local_get(VALUES + index + 1)
            .local_set(VALUES + index);
    }
    out.c().local_get(ROTATE).local_set(VALUES + 7);
    for local in [VALID, DIRTY] {
        out.c()
            .local_get(local)
            .i32_const(1)
            .i32_shr_u()
            .local_get(local)
            .i32_const(7)
            .i32_shl()
            .i32_or()
            .i32_const(255)
            .i32_and()
            .local_set(local);
    }
}

// this conversion is only reached for exact normal binary64 values or signed zero.
pub(super) fn materialize_slot(out: &mut Output<'_>, index: u8, pointer: u32) {
    emit_slot(
        &mut out.c(),
        index,
        VALUES + u32::from(index),
        DIRTY,
        pointer,
        EXPONENT,
        PACKED,
    );
}

fn emit_slot(
    code: &mut InstructionSink<'_>,
    index: u8,
    value: u32,
    dirty: u32,
    pointer: u32,
    exponent: u32,
    packed: u32,
) {
    code.local_get(dirty)
        .i32_const(1 << index)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(value)
        .i64_reinterpret_f64()
        .local_tee(packed)
        .i64_const(i64::MAX)
        .i64_and()
        .i64_eqz()
        .if_(BlockType::Empty)
        .local_get(pointer)
        .i64_const(0)
        .i64_store(mem(0, 0))
        .i32_const(0)
        .local_set(exponent)
        .else_()
        .local_get(pointer)
        .local_get(packed)
        .i64_const(0x000f_ffff_ffff_ffff)
        .i64_and()
        .i64_const(0x0010_0000_0000_0000)
        .i64_or()
        .i64_const(11)
        .i64_shl()
        .i64_store(mem(0, 0))
        .local_get(packed)
        .i64_const(52)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(2047)
        .i32_and()
        .i32_const(15360)
        .i32_add()
        .local_set(exponent)
        .end()
        .local_get(pointer)
        .local_get(exponent)
        .local_get(packed)
        .i64_const(48)
        .i64_shr_u()
        .i32_wrap_i64()
        .i32_const(0x8000)
        .i32_and()
        .i32_or()
        .i32_store16(mem(8, 0))
        .local_get(dirty)
        .i32_const(!(1 << index))
        .i32_and()
        .local_set(dirty)
        .end();
}

// params are st0..st7, bank, dirty, status, tag, top, fip, fdp, opcode, cs, ds.
pub(in crate::cpu::core::emitter) fn emit_materializer() -> Function {
    let mut function = Function::new([(2, ValType::I32), (1, ValType::I64)]);
    let mut code = function.instructions();
    for index in 0..8 {
        code.local_get(8)
            .local_get(12)
            .i32_const(i32::from(index))
            .i32_add()
            .i32_const(7)
            .i32_and()
            .i32_const(10)
            .i32_mul()
            .i32_add()
            .i32_const(20)
            .i32_add()
            .local_set(18);
        emit_slot(&mut code, index, u32::from(index), 9, 18, 19, 20);
    }
    for (offset, local, wide) in [
        (2, 10, false),
        (4, 11, false),
        (6, 15, false),
        (8, 13, true),
        (12, 14, true),
        (16, 16, false),
        (18, 17, false),
    ] {
        code.local_get(8).local_get(local);
        if wide {
            code.i32_store(mem(offset, 2));
        } else {
            code.i32_store16(mem(offset, 1));
        }
    }
    code.end();
    function
}

pub(in crate::cpu::core::emitter) fn materialize(out: &mut Output<'_>) {
    resolve_status(out);
    out.c().local_get(ACTIVE).if_(BlockType::Empty);
    for index in 0..8 {
        out.c().local_get(VALUES + index);
    }
    for local in [BANK, DIRTY, STATUS, TAG, TOP, FIP, FDP, OPCODE, CS, DS] {
        out.c().local_get(local);
    }
    let function = out.internal(7);
    out.c().call(function).i32_const(0).local_set(DIRTY).end();
}

pub(super) fn invalidate(out: &mut Output<'_>) {
    materialize(out);
    for local in [ACTIVE, VALID, DIRTY, super::double::PENDING] {
        out.c().i32_const(0).local_set(local);
    }
}
