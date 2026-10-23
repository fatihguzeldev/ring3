pub(in crate::cpu::core) mod authority;
pub(in crate::cpu::core) mod cache;
pub(in crate::cpu::core) mod groups;

use super::emitter::{ADDRESS, CTX};
use wasm_encoder::{BlockType, Function, InstructionSink, MemArg, ValType};

pub(super) const MAPPINGS: u64 = 56;
pub(super) const PERMISSIONS: u64 = 60;
pub(super) const BACKING: u64 = 64;
pub(super) const BACKING_PAGES: u64 = 68;
pub(super) const EXECUTABLE_VIEWS: u64 = 72;
pub(super) const PRIVATE_SLOTS: u64 = 76;
pub(super) const TRANSACTION_VERSION: u64 = 80;
pub(super) const PAGE_VERSIONS: u64 = 84;
pub(super) const CODE_VERSION: u64 = 88;
pub(super) const EXPECTED_CODE_VERSION: u64 = 96;

const MAPPING_BASE: u32 = 29;
const PERMISSION_BASE: u32 = 30;
const BACKING_BASE: u32 = 31;
const PAGE_COUNT: u32 = 32;
const SLOT: u32 = 33;
const POINTER: u32 = 34;
const PREPARED_SLOT: u32 = 35;
const PREPARED_POINTER: u32 = 36;

fn arg(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}

fn load_field(code: &mut InstructionSink<'_>, offset: u64) {
    code.local_get(CTX).i32_load(arg(offset, 2));
}

pub(super) fn load_view(code: &mut InstructionSink<'_>) {
    // allocation bases stay fixed during the exclusive cpu lifetime; entries remain live.
    for (offset, local) in [
        (MAPPINGS, MAPPING_BASE),
        (PERMISSIONS, PERMISSION_BASE),
        (BACKING, BACKING_BASE),
        (BACKING_PAGES, PAGE_COUNT),
    ] {
        load_field(code, offset);
        code.local_set(local);
    }
}

// the enclosing empty block branches to the canonical slow access.
fn geometry(
    code: &mut InstructionSink<'_>,
    width: u32,
    permission: i32,
    miss_index: u32,
    failure_depth: u32,
) {
    debug_assert!(matches!(width, 1 | 2 | 4));
    if width != 1 {
        code.local_get(ADDRESS)
            .i32_const(4095)
            .i32_and()
            .i32_const((4096 - width) as i32)
            .i32_gt_u()
            .br_if(failure_depth);
    }
    cache::translate_at(code, permission, miss_index, failure_depth);
}

pub(super) fn read(
    code: &mut InstructionSink<'_>,
    encoded_width: u32,
    slow_index: u32,
    miss_index: u32,
    prepare: bool,
    grouped: bool,
) {
    let width = encoded_width & 0xff;
    let modify = encoded_width & 0x100 != 0;
    debug_assert!(matches!(width, 1 | 2 | 4) && encoded_width & !0x1ff == 0);
    debug_assert!(!prepare || modify);
    if prepare {
        code.i32_const(0).local_set(PREPARED_SLOT);
    }
    code.block(BlockType::Result(ValType::I64))
        .block(BlockType::Empty);
    if grouped {
        code.local_get(groups::ADMITTED)
            .i32_eqz()
            .if_(BlockType::Empty);
    }
    geometry(
        code,
        width,
        if modify { 3 } else { 1 },
        miss_index,
        u32::from(grouped),
    );
    if grouped {
        code.end();
    }
    if prepare {
        code.local_get(SLOT)
            .i32_const(1)
            .i32_add()
            .local_set(PREPARED_SLOT)
            .local_get(POINTER)
            .local_set(PREPARED_POINTER);
    }
    code.local_get(POINTER);
    match width {
        1 => code.i32_load8_u(arg(0, 0)),
        2 => code.i32_load16_u(arg(0, 0)),
        4 => code.i32_load(arg(0, 0)),
        _ => unreachable!(),
    };
    code.i64_extend_i32_u().br(1).end();
    authority::flush(code);
    code.local_get(CTX)
        .local_get(ADDRESS)
        .i32_const(encoded_width as i32)
        .call(slow_index);
    cache::invalidate(code);
    authority::load(code);
    code.end();
}

pub(super) fn write(
    code: &mut InstructionSink<'_>,
    width: u32,
    source: u32,
    commit_index: u32,
    slow_index: u32,
    miss_index: u32,
    prepared: bool,
    group_fallback: bool,
) {
    debug_assert!(matches!(width, 1 | 2 | 4));
    code.block(BlockType::Result(ValType::I32))
        .block(BlockType::Empty);
    if prepared {
        if group_fallback {
            code.local_get(PREPARED_SLOT)
                .if_(BlockType::Empty)
                .local_get(PREPARED_SLOT)
                .i32_const(1)
                .i32_sub()
                .local_set(SLOT)
                .local_get(PREPARED_POINTER)
                .local_set(POINTER)
                .else_();
            geometry(code, width, 2, miss_index, 1);
            code.end();
        } else {
            // slow reads can change mappings, so their writes repeat canonical preflight.
            code.local_get(PREPARED_SLOT)
                .i32_eqz()
                .br_if(0)
                .local_get(PREPARED_SLOT)
                .i32_const(1)
                .i32_sub()
                .local_set(SLOT)
                .local_get(PREPARED_POINTER)
                .local_set(POINTER);
        }
    } else {
        geometry(code, width, 2, miss_index, 0);
    }

    code.local_get(CTX)
        .local_get(ADDRESS)
        .local_get(source)
        .local_get(SLOT)
        .local_get(POINTER)
        .local_get(groups::ADMITTED)
        .local_get(authority::SEQUENCE)
        .call(commit_index)
        .local_set(authority::SEQUENCE)
        .br_if(0)
        .i32_const(0)
        .br(1)
        .end();
    authority::flush(code);
    code.local_get(CTX)
        .local_get(ADDRESS)
        .i32_const(width as i32)
        .local_get(source)
        .call(slow_index);
    cache::invalidate(code);
    authority::load(code);
    code.end();
}

pub(super) fn emit_store(width: u32) -> Function {
    // params: context, address, value, slot, pointer, capability, sequence.
    // only an admitted nonexec/noncow page skips stable authority table reads;
    // the payload, overflow, sequence and page stamp remain one common body.
    let mut function = Function::new([(1, ValType::I32), (1, ValType::I64)]);
    let mut code = function.instructions();
    code.block(BlockType::Empty)
        .local_get(5)
        .i32_const(groups::PLAIN_WRITE)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    // a writable virtual view can still alias executable physical bytes.
    load_field(&mut code, EXECUTABLE_VIEWS);
    code.local_get(3)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(1);
    code.block(BlockType::Empty);
    load_field(&mut code, PRIVATE_SLOTS);
    code.local_tee(7).i32_eqz().br_if(0);
    code.local_get(7)
        .local_get(1)
        .i32_const(12)
        .i32_shr_u()
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(2);
    code.end().end();

    code.local_get(6)
        .i64_const(-1)
        .i64_eq()
        .br_if(0)
        .local_get(6)
        .i64_const(1)
        .i64_add()
        .local_set(8);
    code.local_get(4).local_get(2);
    match width {
        1 => code.i32_store8(arg(0, 0)),
        2 => code.i32_store16(arg(0, 0)),
        4 => code.i32_store(arg(0, 0)),
        _ => unreachable!(),
    };
    load_field(&mut code, PAGE_VERSIONS);
    code.local_get(3)
        .i32_const(4)
        .i32_shl()
        .i32_add()
        .local_get(8)
        .i64_store(arg(8, 3));
    code.i32_const(0)
        .local_get(8)
        .return_()
        .end()
        .i32_const(1)
        .local_get(6)
        .end();
    function
}
