//! The ordinary cross-fragment route is generated code under an exclusive lease.
//! Native resolution remains the single miss/invalid-source/capture path.
use crate::cpu::core::{
    continuation as d,
    emitter::{link, mem},
    trace,
};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const HEADER: u32 = 4;
const ROW: u32 = 5;
const LEFT: u32 = 6;
const CANDIDATE: u32 = 7;
const INDEX: u32 = 8;
const OWNER: u32 = 9;
const SLOT: u32 = 10;
const OLDER: u32 = 11;
const NEWER: u32 = 12;
const NEWEST: u32 = 13;
const ARENA: u32 = 14;
const PORT: u32 = 15;
const BYPASS: u32 = 16;

fn load(c: &mut InstructionSink<'_>, base: u32, offset: u64) {
    c.local_get(base).i32_load(mem(offset, 2));
}
fn load64(c: &mut InstructionSink<'_>, base: u32, offset: u64) {
    c.local_get(base).i64_load(mem(offset, 3));
}
fn miss(c: &mut InstructionSink<'_>) {
    c.if_(BlockType::Empty).i32_const(0).return_().end();
}
fn owner(c: &mut InstructionSink<'_>, index: u32) {
    load(c, HEADER, d::OWNERS);
    c.local_get(index)
        .i32_const(1)
        .i32_sub()
        .i32_const(d::OWNER_SIZE as i32)
        .i32_mul()
        .i32_add();
}
fn store_local(c: &mut InstructionSink<'_>, offset: u64, local: u32) {
    c.local_get(HEADER)
        .local_get(local)
        .i32_store(mem(offset, 2));
}

fn sample(c: &mut InstructionSink<'_>, candidate: u64) {
    if candidate == 0 {
        return;
    } // window roots already own a fused body.
    load64(c, OWNER, d::OWNER_ID);
    load64(c, HEADER, d::CURRENT_ID);
    c.i64_ne().if_(BlockType::Empty);
    load(c, ARENA, trace::SAMPLE_COUNTDOWN);
    c.i32_const(1)
        .i32_sub()
        .local_set(LEFT)
        .local_get(ARENA)
        .local_get(LEFT)
        .i32_store(mem(trace::SAMPLE_COUNTDOWN, 2))
        .local_get(LEFT)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(ARENA)
        .i32_const(trace::SAMPLE_INTERVAL as i32)
        .i32_store(mem(trace::SAMPLE_COUNTDOWN, 2))
        .local_get(ARENA)
        .i32_const(1)
        .i32_store(mem(trace::SAMPLE_PENDING, 2))
        .i32_const(0)
        .return_()
        .end()
        .end();
}

pub(super) fn emit(state_base: u32) -> Function {
    // Parameters: Context, actual PC, actual retired count, available work.
    let mut f = Function::new([(12, ValType::I32), (1, ValType::I64)]);
    let mut c = f.instructions();
    c.global_get(state_base + link::CONTINUATION)
        .local_tee(HEADER)
        .i32_eqz();
    miss(&mut c);
    load(&mut c, HEADER, d::FORMAT);
    c.i32_const(d::FORMAT_VERSION as i32).i32_ne();
    miss(&mut c);
    load(&mut c, HEADER, d::ACTIVE);
    c.i32_const(1).i32_ne();
    miss(&mut c);
    load(&mut c, HEADER, d::CONTEXT);
    c.local_get(0).i32_ne();
    miss(&mut c);
    load64(&mut c, HEADER, d::CURRENT_NONCE);
    load64(&mut c, HEADER, d::NONCE);
    c.i64_ne();
    miss(&mut c);
    load(&mut c, HEADER, d::ROWS);
    c.i32_eqz();
    miss(&mut c);
    load(&mut c, HEADER, d::OWNERS);
    c.i32_eqz();
    miss(&mut c);
    load(&mut c, HEADER, d::OWNER_LEN);
    c.i32_eqz();
    miss(&mut c);
    load(&mut c, HEADER, d::TRACE_ARENA);
    c.local_tee(ARENA).i32_eqz();
    miss(&mut c);
    load(&mut c, ARENA, trace::ACTIVE);
    miss(&mut c);
    c.i64_const(0).local_set(BYPASS);
    load(&mut c, ARENA, trace::BYPASS_PENDING);
    c.i32_const(1).i32_eq();
    load(&mut c, ARENA, trace::BYPASS_RETIRED);
    c.local_get(2).i32_eq().i32_and().if_(BlockType::Empty);
    load64(&mut c, ARENA, trace::BYPASS_ID);
    c.local_set(BYPASS).end();
    load(&mut c, HEADER, d::ROW_MASK);
    c.i32_const(1).i32_add().local_set(LEFT);
    c.local_get(1)
        .local_get(1)
        .i32_const(16)
        .i32_shr_u()
        .i32_xor()
        .i32_const(d::HASH_MULTIPLIER as i32)
        .i32_mul();
    load(&mut c, HEADER, d::ROW_MASK);
    c.i32_and().local_set(ROW);
    c.loop_(BlockType::Empty);
    load(&mut c, HEADER, d::ROWS);
    c.local_get(ROW)
        .i32_const(d::ROW_SIZE as i32)
        .i32_mul()
        .i32_add()
        .local_set(CANDIDATE);
    load(&mut c, CANDIDATE, d::ROW_STATE);
    c.i32_eqz();
    miss(&mut c);
    load(&mut c, CANDIDATE, d::ROW_STATE);
    c.i32_const(d::PRESENT as i32).i32_eq();
    load(&mut c, CANDIDATE, d::ROW_PC);
    c.local_get(1).i32_eq().i32_and().if_(BlockType::Empty);
    // Priority is window, family, baseline. A higher-priority present but cold
    // or invalid candidate must resolve natively rather than silently falling through.
    for candidate in 0..3 {
        c.local_get(CANDIDATE)
            .i32_const((d::CANDIDATES + candidate * d::CANDIDATE_SIZE) as i32)
            .i32_add()
            .local_set(OWNER);
        load64(&mut c, OWNER, d::CANDIDATE_ID);
        c.i64_eqz().i32_eqz();
        load64(&mut c, OWNER, d::CANDIDATE_ID);
        c.local_get(BYPASS).i64_ne().i32_and().if_(BlockType::Empty);
        load(&mut c, OWNER, d::CANDIDATE_WORK);
        c.i32_eqz();
        miss(&mut c);
        load(&mut c, OWNER, d::CANDIDATE_WORK);
        c.local_get(3).i32_gt_u();
        miss(&mut c);
        load(&mut c, OWNER, d::CANDIDATE_FLAGS);
        c.i32_const(d::NON_HOT as i32).i32_and();
        miss(&mut c);
        load(&mut c, OWNER, d::CANDIDATE_PORT);
        c.local_tee(PORT).i32_const(-1).i32_eq();
        miss(&mut c);
        load(&mut c, OWNER, d::CANDIDATE_INDEX);
        c.local_tee(INDEX).i32_eqz();
        miss(&mut c);
        c.local_get(INDEX);
        load(&mut c, HEADER, d::OWNER_LEN);
        c.i32_gt_u();
        miss(&mut c);
        owner(&mut c, INDEX);
        c.local_set(OWNER);
        load64(&mut c, OWNER, d::OWNER_ID);
        load64(
            &mut c,
            CANDIDATE,
            d::CANDIDATES + candidate * d::CANDIDATE_SIZE + d::CANDIDATE_ID,
        );
        c.i64_ne();
        miss(&mut c);
        load(&mut c, OWNER, d::OWNER_FLAGS);
        c.i32_const(d::BOUND as i32).i32_and().i32_eqz();
        miss(&mut c);
        if candidate == 0 {
            load(&mut c, CANDIDATE, d::CANDIDATES + d::CANDIDATE_FLAGS);
            c.i32_const(d::WINDOW_ROOT as i32).i32_and().i32_eqz();
            miss(&mut c);
        }
        c.local_get(0)
            .i32_load(mem(crate::cpu::core::memory::CODE_VERSION, 2))
            .local_tee(SLOT)
            .i32_eqz();
        miss(&mut c);
        load64(&mut c, SLOT, 0);
        load64(&mut c, OWNER, d::OWNER_VALIDATION);
        c.i64_ne();
        miss(&mut c);
        load(&mut c, OWNER, d::OWNER_SLOT);
        c.local_tee(SLOT).i32_eqz();
        miss(&mut c);
        c.local_get(SLOT);
        load(&mut c, HEADER, d::SLOT_LEN);
        c.i32_ge_u();
        miss(&mut c);
        load(&mut c, HEADER, d::LINK_OWNERS);
        c.local_get(SLOT)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .i64_load(mem(0, 3));
        load64(&mut c, OWNER, d::OWNER_ID);
        c.i64_ne();
        miss(&mut c);
        load(&mut c, HEADER, d::SLOT_REGIONS);
        c.local_get(SLOT)
            .i32_const(2)
            .i32_shl()
            .i32_add()
            .i32_load(mem(0, 2));
        c.local_get(INDEX).i32_const(1).i32_sub().i32_ne();
        miss(&mut c);
        load(&mut c, HEADER, d::SLOT_GENERATIONS);
        c.local_get(SLOT)
            .i32_const(3)
            .i32_shl()
            .i32_add()
            .i64_load(mem(0, 3));
        load64(&mut c, OWNER, d::OWNER_GENERATION);
        c.i64_ne();
        miss(&mut c);
        // The shared intrusive LRU is the sole eviction order. Validate all
        // neighboring indices before making its bounded pointer writes.
        load(&mut c, HEADER, d::NEWEST);
        c.local_tee(NEWEST).i32_eqz();
        miss(&mut c);
        c.local_get(NEWEST);
        load(&mut c, HEADER, d::OWNER_LEN);
        c.i32_gt_u();
        miss(&mut c);
        c.local_get(NEWEST)
            .local_get(INDEX)
            .i32_ne()
            .if_(BlockType::Empty);
        load(&mut c, OWNER, d::OWNER_OLDER);
        c.local_set(OLDER);
        load(&mut c, OWNER, d::OWNER_NEWER);
        c.local_tee(NEWER).i32_eqz();
        miss(&mut c);
        c.local_get(NEWER);
        load(&mut c, HEADER, d::OWNER_LEN);
        c.i32_gt_u();
        miss(&mut c);
        owner(&mut c, NEWER);
        c.i32_load(mem(d::OWNER_OLDER, 2)).local_get(INDEX).i32_ne();
        miss(&mut c);
        c.local_get(OLDER).if_(BlockType::Empty);
        c.local_get(OLDER);
        load(&mut c, HEADER, d::OWNER_LEN);
        c.i32_gt_u();
        miss(&mut c);
        owner(&mut c, OLDER);
        c.i32_load(mem(d::OWNER_NEWER, 2)).local_get(INDEX).i32_ne();
        miss(&mut c);
        c.else_();
        load(&mut c, HEADER, d::OLDEST);
        c.local_get(INDEX).i32_ne();
        miss(&mut c);
        c.end();
        owner(&mut c, NEWEST);
        c.i32_load(mem(d::OWNER_NEWER, 2));
        miss(&mut c);
        // sample only a completely validated route, before any LRU/ticket writes.
        sample(&mut c, candidate);
        c.local_get(OLDER).if_(BlockType::Empty);
        owner(&mut c, OLDER);
        c.local_get(NEWER).i32_store(mem(d::OWNER_NEWER, 2));
        c.else_();
        store_local(&mut c, d::OLDEST, NEWER);
        c.end();
        owner(&mut c, NEWER);
        c.local_get(OLDER).i32_store(mem(d::OWNER_OLDER, 2));
        owner(&mut c, NEWEST);
        c.local_get(INDEX).i32_store(mem(d::OWNER_NEWER, 2));
        c.local_get(OWNER)
            .local_get(NEWEST)
            .i32_store(mem(d::OWNER_OLDER, 2));
        c.local_get(OWNER)
            .i32_const(0)
            .i32_store(mem(d::OWNER_NEWER, 2));
        store_local(&mut c, d::NEWEST, INDEX);
        c.else_();
        sample(&mut c, candidate);
        c.end();
        store_local(&mut c, d::CURRENT_INDEX, INDEX);
        store_local(&mut c, d::CURRENT_SLOT, SLOT);
        store_local(&mut c, d::CURRENT_ENTRY, 1);
        store_local(&mut c, d::CURRENT_PORT, PORT);
        for (target, source) in [
            (d::CURRENT_ID, d::OWNER_ID),
            (d::CURRENT_GENERATION, d::OWNER_GENERATION),
        ] {
            c.local_get(HEADER);
            load64(&mut c, OWNER, source);
            c.i64_store(mem(target, 3));
        }
        c.local_get(HEADER);
        load64(&mut c, HEADER, d::NONCE);
        c.i64_store(mem(d::CURRENT_NONCE, 3));
        c.local_get(HEADER).i32_const(0).i32_store(mem(d::FLAGS, 2));
        c.local_get(0);
        load64(&mut c, OWNER, d::OWNER_ID);
        c.i64_store(mem(120, 3));
        c.local_get(0);
        load64(&mut c, OWNER, d::OWNER_VALIDATION);
        c.i64_store(mem(crate::cpu::core::memory::EXPECTED_CODE_VERSION, 3));
        c.local_get(PORT)
            .i32_const(1)
            .i32_add()
            .global_set(state_base + link::PORT);
        c.local_get(SLOT).return_().end();
    }
    c.i32_const(0).return_().end();
    c.local_get(LEFT)
        .i32_const(1)
        .i32_sub()
        .local_tee(LEFT)
        .i32_eqz();
    miss(&mut c);
    c.local_get(ROW).i32_const(1).i32_add();
    load(&mut c, HEADER, d::ROW_MASK);
    c.i32_and().local_set(ROW);
    c.br(0).end().unreachable().end();
    f
}
