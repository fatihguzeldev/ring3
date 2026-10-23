use super::super::{CTX, LOGICAL_ADDRESS, Output, mem};
use crate::cpu::core::fp_pool as p;
use crate::cpu::x86::ir::EffectiveAddress;
use wasm_encoder::{BlockType, Function};

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

// The frame owns only demanded payloads. Physical pool records remain authoritative
// for every untouched slot, including empty-stack ghosts.
pub(super) fn enter(out: &mut Output<'_>) {
    out.c()
        .local_get(ACTIVE)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(super::BANK_POINTER, 2))
        .local_tee(BANK)
        .i32_eqz()
        .br_if(1);
    super::pool::initialize(out);
    for (offset, local) in [
        (p::CONTROL, CONTROL),
        (p::STATUS, STATUS),
        (p::TAG, TAG),
        (p::TOP, TOP),
        (p::OPCODE, OPCODE),
        (p::FIP, FIP),
        (p::FDP, FDP),
        (p::CS, CS),
        (p::DS, DS),
        (p::PENDING, super::double::PENDING),
    ] {
        out.c()
            .local_get(super::pool::POINTER)
            .i32_load(mem(offset, 2))
            .local_set(local);
    }
    for (offset, local) in [
        (p::PENDING_LEFT, super::double::PENDING_LEFT),
        (p::PENDING_RIGHT, super::double::PENDING_RIGHT),
        (p::PENDING_RESULT, super::double::PENDING_RESULT),
    ] {
        out.c()
            .local_get(super::pool::POINTER)
            .f64_load(mem(offset, 3))
            .local_set(local);
    }
    out.c()
        .i32_const(0)
        .local_set(VALID)
        .i32_const(0)
        .local_set(DIRTY)
        .i32_const(0)
        .local_set(super::pool::LOADED)
        .i32_const(0)
        .local_set(super::pool::ADMITTED)
        .i32_const(1)
        .local_set(ACTIVE)
        .end();
    for (from, to) in [(BANK, 37), (STATUS, 41)] {
        out.c().local_get(from).local_set(to);
    }
    out.c()
        .local_get(CONTROL)
        .i32_const(63)
        .i32_and()
        .i32_const(63)
        .i32_ne()
        .br_if(0);
}

pub(super) fn status(out: &mut Output<'_>) {
    resolve_status(out);
    let delta = out
        .before_frame
        .map_or(0, |frame| out.plan.shape(frame).top_delta);
    out.c()
        .local_get(ACTIVE)
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .local_get(STATUS)
        .i32_const(!0x3800)
        .i32_and()
        .local_get(TOP)
        .i32_const(i32::from(delta))
        .i32_add()
        .i32_const(7)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .else_();
    super::pool::status(out);
    out.c().end().local_set(41);
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

pub(super) fn commit(out: &mut Output<'_>, _opcode: u16, address: Option<EffectiveAddress>) {
    out.c().local_get(41).local_set(STATUS);
    if address.is_some() {
        // A later GP write must not change the historical x87 data pointer.
        out.c().local_get(LOGICAL_ADDRESS).local_set(FDP);
    }
}

pub(super) fn valid(out: &mut Output<'_>, index: u8) {
    let cell = super::frame::input_cell(out, index);
    out.c().local_get(VALID).i32_const(1 << cell).i32_and();
}

pub(super) fn load(out: &mut Output<'_>, index: u8, destination: u32) {
    let cell = super::frame::input_cell(out, index);
    out.c()
        .local_get(VALUES + u32::from(cell))
        .local_set(destination);
}

pub(super) fn remember(out: &mut Output<'_>, index: u8, source: u32) {
    let cell = super::frame::input_cell(out, index);
    out.c()
        .local_get(source)
        .local_set(VALUES + u32::from(cell))
        .local_get(VALID)
        .i32_const(1 << cell)
        .i32_or()
        .local_set(VALID);
    super::pool::mark_loaded(out, cell);
    super::pool::remember_input(out, index, source);
}

pub(super) fn write(out: &mut Output<'_>, _index: u8, source: u32) {
    let cell = super::frame::result_cell(out);
    out.c()
        .local_get(source)
        .local_set(VALUES + u32::from(cell))
        .local_get(VALID)
        .i32_const(1 << cell)
        .i32_or()
        .local_set(VALID);
    super::pool::mark_loaded(out, cell);
    // Dirty destinations, TOP and tags belong to the compile-time FrameState.
}

pub(in crate::cpu::core::emitter) fn materialize(out: &mut Output<'_>) {
    out.c().local_get(CTX).i32_load(mem(super::BANK_POINTER, 2));
    let function = out.internal(7);
    out.c().call(function);
}

pub(super) fn invalidate(out: &mut Output<'_>) {
    materialize(out);
    for local in [
        ACTIVE,
        VALID,
        DIRTY,
        super::pool::LOADED,
        super::pool::ADMITTED,
        super::double::PENDING,
    ] {
        out.c().i32_const(0).local_set(local);
    }
}
