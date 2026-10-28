use super::{ADDRESS, BACKING, BACKING_PAGES, CTX, MAPPINGS, PERMISSIONS, POINTER, SLOT, arg};
use wasm_encoder::{BlockType, Function, InstructionSink, ValType};

const PAGE_KEY: u32 = 104;
const CACHED_SLOT: u32 = 105;
const PAGE_BASE: u32 = 106;
const CACHED_PERMISSIONS: u32 = 107;

#[derive(Clone, Copy)]
pub(in crate::cpu::core) struct Locals {
    pub secondary: u32,
}

pub(in crate::cpu::core) fn invalidate(code: &mut InstructionSink<'_>, locals: Locals) {
    code.i32_const(0)
        .local_set(PAGE_KEY)
        .i32_const(0)
        .local_set(locals.secondary);
    super::groups::invalidate(code);
}

fn page(code: &mut InstructionSink<'_>) {
    code.local_get(ADDRESS).i32_const(12).i32_shr_u();
}

// the caller checks the span first; rejection exits its enclosing canonical-access block.
pub(in crate::cpu::core) fn translate_at(
    code: &mut InstructionSink<'_>,
    permission: i32,
    miss_index: u32,
    failure_depth: u32,
    locals: Locals,
) {
    debug_assert!(matches!(permission, 1..=3));
    code.local_get(PAGE_KEY);
    page(code);
    code.i32_const(1)
        .i32_add()
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(CACHED_PERMISSIONS)
        .i32_const(permission)
        .i32_and()
        .i32_const(permission)
        .i32_ne()
        .br_if(1 + failure_depth)
        .local_get(CACHED_SLOT)
        .local_set(SLOT)
        .local_get(PAGE_BASE)
        .local_set(POINTER)
        .else_()
        .local_get(locals.secondary);
    page(code);
    code.i32_const(1)
        .i32_add()
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(locals.secondary + 3)
        .i32_const(permission)
        .i32_and()
        .i32_const(permission)
        .i32_ne()
        .br_if(2 + failure_depth)
        .local_get(locals.secondary + 1)
        .local_set(SLOT)
        .local_get(locals.secondary + 2)
        .local_set(POINTER)
        .else_();
    for (source, destination) in [
        (PAGE_KEY, locals.secondary),
        (CACHED_SLOT, locals.secondary + 1),
        (PAGE_BASE, locals.secondary + 2),
        (CACHED_PERMISSIONS, locals.secondary + 3),
    ] {
        code.local_get(source).local_set(destination);
    }
    code.i32_const(0)
        .local_set(PAGE_KEY)
        .local_get(CTX)
        .local_get(ADDRESS)
        .i32_const(permission)
        .call(miss_index)
        .local_set(CACHED_PERMISSIONS)
        .local_set(PAGE_BASE)
        .local_tee(SLOT)
        .i32_const(-1)
        .i32_eq()
        .br_if(2 + failure_depth)
        .local_get(SLOT)
        .local_set(CACHED_SLOT);
    page(code);
    code.i32_const(1)
        .i32_add()
        .local_set(PAGE_KEY)
        .local_get(PAGE_BASE)
        .local_set(POINTER)
        .end()
        .end();
    code.local_get(POINTER)
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .local_set(POINTER);
}

// params: context, address, permission; results: slot, physical page base, full permissions.
pub(in crate::cpu::core) fn emit_translator() -> Function {
    let mut function = Function::new([(2, ValType::I32)]);
    let mut code = function.instructions();
    code.block(BlockType::Empty)
        .local_get(0)
        .i32_load(arg(MAPPINGS, 2))
        .local_get(1)
        .i32_const(12)
        .i32_shr_u()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .i32_const(1)
        .i32_sub()
        .local_tee(3)
        .local_get(0)
        .i32_load(arg(BACKING_PAGES, 2))
        .i32_ge_u()
        .br_if(0)
        .local_get(0)
        .i32_load(arg(PERMISSIONS, 2))
        .local_get(1)
        .i32_const(12)
        .i32_shr_u()
        .i32_add()
        .i32_load8_u(arg(0, 0))
        .local_tee(4)
        .local_get(2)
        .i32_and()
        .local_get(2)
        .i32_ne()
        .br_if(0)
        .local_get(3)
        .local_get(0)
        .i32_load(arg(BACKING, 2))
        .local_get(3)
        .i32_const(12)
        .i32_shl()
        .i32_add()
        .local_get(4)
        .return_()
        .end()
        .i32_const(-1)
        .i32_const(0)
        .i32_const(0)
        .end();
    function
}
