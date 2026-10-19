use super::{ReadViewLocals, arg, bounded};
use wasm_encoder::{BlockType, InstructionSink};

pub(super) fn initialize(code: &mut InstructionSink<'_>, locals: ReadViewLocals) {
    // mapping temporarily owns the raw optional getter result. publish pointer
    // only after every geometry check succeeds; zero disables this call's view.
    code.i32_const(0)
        .local_set(locals.pointer)
        .block(BlockType::Empty)
        .local_get(locals.mapping)
        .i32_eqz()
        .br_if(0)
        .memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .local_set(locals.memory_bytes);
    bounded(code, locals.mapping, 24, locals.memory_bytes);
    for (offset, local) in fields(locals) {
        code.local_get(locals.mapping)
            .i32_load(arg(offset, 2))
            .local_set(local);
    }
    code.local_get(locals.pages)
        .i32_const(1)
        .i32_sub()
        .i32_const(crate::memory::MAX_RESIDENT_PAGES as i32 - 1)
        .i32_gt_u()
        .br_if(0);
    for local in [locals.mappings, locals.permissions, locals.backing] {
        code.local_get(local).i32_eqz().br_if(0);
    }
    bounded(code, locals.mappings, 4 << 20, locals.memory_bytes);
    bounded(code, locals.permissions, 1 << 20, locals.memory_bytes);
    code.local_get(locals.backing)
        .i64_extend_i32_u()
        .local_get(locals.pages)
        .i64_extend_i32_u()
        .i64_const(12)
        .i64_shl()
        .i64_add()
        .local_get(locals.memory_bytes)
        .i64_gt_u()
        .br_if(0)
        .local_get(locals.mapping)
        .local_set(locals.pointer)
        .end();
}

pub(super) fn require_unchanged(code: &mut InstructionSink<'_>, locals: ReadViewLocals) {
    // the enclosing attempt owns label zero. changed geometry uses the native
    // fallback, never cached tables; rights and mappings remain fresh per load.
    for (offset, local) in fields(locals) {
        code.local_get(locals.pointer)
            .i32_load(arg(offset, 2))
            .local_get(local)
            .i32_ne()
            .br_if(0);
    }
}

fn fields(locals: ReadViewLocals) -> [(u64, u32); 4] {
    [
        (8, locals.mappings),
        (12, locals.permissions),
        (16, locals.backing),
        (20, locals.pages),
    ]
}
