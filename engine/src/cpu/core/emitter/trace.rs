use super::{CTX, Output, RETIRED, exits, link, mem};
use crate::cpu::core::{StopReason, trace};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

pub(super) fn parameters() -> [ValType; 6] {
    [
        ValType::I32,
        ValType::I64,
        ValType::I32,
        ValType::I32,
        ValType::I32,
        ValType::I32,
    ]
}

// Every compiled owner records only while a bounded capture is active. The
// actual owner identity and port preserve source authority across nested traces.
pub(super) fn record_block(out: &mut Output<'_>, entry: u32, end: u32, offset: u32) {
    let recorder = out.internal(15);
    out.c()
        .global_get(link::TRACE_ARENA)
        .if_(BlockType::Empty)
        .global_get(link::TRACE_ARENA)
        .i32_load(mem(trace::ACTIVE, 2))
        .if_(BlockType::Empty)
        .global_get(link::TRACE_ARENA)
        .local_get(CTX)
        .i64_load(mem(120, 3))
        .i32_const(entry as i32)
        .i32_const(end as i32)
        .i32_const(offset as i32)
        .i32_const(trace::BLOCK as i32)
        .call(recorder)
        .end()
        .end();
}

pub(super) fn emit_recorder() -> Function {
    const USED: u32 = 6;
    const SLOT: u32 = 7;
    let mut function = Function::new([(2, ValType::I32)]);
    let mut c = function.instructions();
    c.local_get(0)
        .i32_eqz()
        .if_(BlockType::Empty)
        .return_()
        .end();
    c.local_get(0)
        .i32_load(mem(trace::ACTIVE, 2))
        .i32_eqz()
        .if_(BlockType::Empty)
        .return_()
        .end();
    c.local_get(0)
        .i32_load(mem(trace::USED, 2))
        .local_tee(USED)
        .i32_const(trace::CAPACITY as i32)
        .i32_ge_u()
        .local_get(0)
        .i32_load(mem(trace::LIMIT, 2))
        .i32_const(trace::CAPACITY as i32)
        .i32_ne()
        .i32_or()
        .local_get(0)
        .i32_load(mem(trace::OVERFLOW, 2))
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_const(1)
        .i32_store(mem(trace::OVERFLOW, 2))
        .local_get(0)
        .i32_const(0)
        .i32_store(mem(trace::ACTIVE, 2))
        .return_()
        .end();
    c.local_get(0)
        .local_get(USED)
        .i32_const(trace::EVENT_SIZE as i32)
        .i32_mul()
        .i32_add()
        .local_set(SLOT);
    for (offset, parameter) in [(0, 5), (4, 2), (8, 3), (12, 4)] {
        c.local_get(SLOT)
            .local_get(parameter)
            .i32_store(mem(trace::EVENTS + offset, 2));
    }
    c.local_get(SLOT)
        .local_get(1)
        .i64_store(mem(trace::EVENTS + 16, 3));
    c.local_get(0)
        .local_get(USED)
        .i32_const(1)
        .i32_add()
        .local_tee(USED)
        .i32_store(mem(trace::USED, 2));
    c.local_get(USED)
        .i32_const(trace::CAPACITY as i32)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_const(0)
        .i32_store(mem(trace::ACTIVE, 2))
        .end()
        .end();
    function
}

// Side exits stamp the exact retired prefix. Selection skips this immutable
// overlay until an ordinary execution actually advances that counter.
pub(super) fn emit_exit(c: &mut InstructionSink<'_>, identity: u64) {
    c.local_get(exits::REASON)
        .i32_const(StopReason::NeedCode as i32)
        .i32_eq()
        .if_(BlockType::Empty)
        .global_get(link::TRACE_ARENA)
        .if_(BlockType::Empty)
        .global_get(link::TRACE_ARENA)
        .i64_const(identity as i64)
        .i64_store(mem(trace::BYPASS_ID, 3))
        .global_get(link::TRACE_ARENA)
        .local_get(RETIRED)
        .i32_store(mem(trace::BYPASS_RETIRED, 2))
        .global_get(link::TRACE_ARENA)
        .i32_const(1)
        .i32_store(mem(trace::BYPASS_PENDING, 2))
        .end()
        .end();
}
