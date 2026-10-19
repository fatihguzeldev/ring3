use crate::cpu::dbt::wasm::{
    imports::Imports,
    locals::{ADDRESS, HELPER_STATUS, STATE_PTR},
};
use wasm_encoder::{BlockType, InstructionSink, MemArg};

pub(in crate::cpu::dbt::wasm) const INTEGER_LOCALS: u32 = 7;

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) struct ReadViewLocals {
    pointer: u32,
    mappings: u32,
    permissions: u32,
    backing: u32,
    pages: u32,
    mapping: u32,
    value: u32,
    memory_bytes: u32,
}

impl ReadViewLocals {
    pub(in crate::cpu::dbt::wasm) const fn new(first: u32) -> Self {
        Self {
            pointer: first,
            mappings: first + 1,
            permissions: first + 2,
            backing: first + 3,
            pages: first + 4,
            mapping: first + 5,
            value: first + 6,
            memory_bytes: first + 7,
        }
    }
}

pub(in crate::cpu::dbt::wasm) fn initialize(code: &mut InstructionSink<'_>, imports: Imports) {
    if let Some(index) = imports.read32_view {
        let locals = imports.read32_view_locals.expect("read view locals");
        code.local_get(
            imports
                .detection
                .expect("resident read view detection")
                .epoch,
        )
        .call(index)
        .local_set(locals.pointer);
    }
}

pub(super) fn load(code: &mut InstructionSink<'_>, imports: Imports) {
    let Some(locals) = imports.read32_view_locals else {
        fallback(code, imports);
        return;
    };
    code.block(BlockType::Empty).block(BlockType::Empty);
    code.local_get(locals.pointer).i32_eqz().br_if(0);
    code.memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .local_set(locals.memory_bytes);
    bounded(code, locals.pointer, 24, locals.memory_bytes);
    code.local_get(locals.pointer)
        .i64_load(arg(0, 3))
        .local_get(
            imports
                .detection
                .expect("resident read view detection")
                .epoch,
        )
        .i64_ne()
        .br_if(0);
    code.local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_const(4092)
        .i32_gt_u()
        .br_if(0);
    for (offset, local) in [
        (8, locals.mappings),
        (12, locals.permissions),
        (16, locals.backing),
        (20, locals.pages),
    ] {
        code.local_get(locals.pointer)
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
        .br_if(0);
    code.local_get(locals.mappings)
        .local_get(ADDRESS)
        .i32_const(12)
        .i32_shr_u()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .local_set(locals.mapping);
    code.local_get(locals.mapping)
        .i32_const(1)
        .i32_sub()
        .local_get(locals.pages)
        .i32_ge_u()
        .br_if(0);
    code.local_get(locals.permissions)
        .local_get(ADDRESS)
        .i32_const(12)
        .i32_shr_u()
        .i32_add()
        .i32_load8_u(arg(0, 0))
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .br_if(0);
    code.local_get(locals.backing)
        .local_get(locals.mapping)
        .i32_const(1)
        .i32_sub()
        .i32_const(12)
        .i32_shl()
        .i32_add()
        .local_get(ADDRESS)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .i32_load(arg(0, 0))
        .local_set(locals.value);
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x1000a),
        (8, 40),
        (12, 0),
        (16, 0),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 4),
    ] {
        code.local_get(STATE_PTR)
            .i32_const(value as i32)
            .i32_store(arg(100 + offset, 2));
    }
    code.local_get(STATE_PTR)
        .local_get(locals.value)
        .i32_store(arg(120, 2));
    code.i32_const(0).local_set(HELPER_STATUS).br(1).end();
    fallback(code, imports);
    code.end();
}

fn bounded(code: &mut InstructionSink<'_>, base: u32, length: i64, memory_bytes: u32) {
    code.local_get(base)
        .i64_extend_i32_u()
        .i64_const(length)
        .i64_add()
        .local_get(memory_bytes)
        .i64_gt_u()
        .br_if(0);
}

fn fallback(code: &mut InstructionSink<'_>, imports: Imports) {
    code.local_get(ADDRESS)
        .call(imports.read.expect("ordinary read import"))
        .local_set(HELPER_STATUS);
}

fn arg(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}

#[cfg(test)]
mod tests;
