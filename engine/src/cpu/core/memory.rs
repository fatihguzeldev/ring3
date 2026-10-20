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

fn arg(offset: u64, align: u32) -> MemArg {
    MemArg {
        offset,
        align,
        memory_index: 0,
    }
}

fn load_field(code: &mut InstructionSink<'_>, offset: u64) {
    code.local_get(0).i32_load(arg(offset, 2));
}

fn page(code: &mut InstructionSink<'_>) {
    code.local_get(1).i32_const(12).i32_shr_u();
}

// the enclosing empty block branches to the canonical slow access.
fn geometry(
    code: &mut InstructionSink<'_>,
    mapping: u32,
    width: u32,
    permission: i32,
    permission_local: Option<u32>,
) {
    code.local_get(width)
        .i32_const(1)
        .i32_eq()
        .local_get(width)
        .i32_const(2)
        .i32_eq()
        .i32_or()
        .local_get(width)
        .i32_const(4)
        .i32_eq()
        .i32_or()
        .i32_eqz()
        .br_if(0);
    code.local_get(1)
        .i32_const(4095)
        .i32_and()
        .i32_const(4096)
        .local_get(width)
        .i32_sub()
        .i32_gt_u()
        .br_if(0);
    load_field(code, MAPPINGS);
    page(code);
    code.i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .i32_const(1)
        .i32_sub()
        .local_tee(mapping);
    load_field(code, BACKING_PAGES);
    code.i32_ge_u().br_if(0);
    load_field(code, PERMISSIONS);
    page(code);
    code.i32_add().i32_load8_u(arg(0, 0));
    if let Some(local) = permission_local {
        code.local_get(local)
            .i32_and()
            .local_get(local)
            .i32_ne()
            .br_if(0);
    } else {
        code.i32_const(permission).i32_and().i32_eqz().br_if(0);
    }
}

fn physical_address(code: &mut InstructionSink<'_>, mapping: u32) {
    load_field(code, BACKING);
    code.local_get(mapping)
        .i32_const(12)
        .i32_shl()
        .i32_add()
        .local_get(1)
        .i32_const(4095)
        .i32_and()
        .i32_add();
}

pub(super) fn emit_read(slow_index: u32) -> Function {
    // bit 8 of the width requests read/write preflight for a modifying instruction.
    let mut function = Function::new([(4, ValType::I32)]);
    let mut code = function.instructions();
    code.block(BlockType::Result(ValType::I64))
        .block(BlockType::Empty);
    code.local_get(2).i32_const(!0x1ff).i32_and().br_if(0);
    code.local_get(2).i32_const(0xff).i32_and().local_set(5);
    code.local_get(2)
        .i32_const(0x100)
        .i32_and()
        .i32_const(7)
        .i32_shr_u()
        .i32_const(1)
        .i32_or()
        .local_set(6);
    geometry(&mut code, 3, 5, 1, Some(6));
    physical_address(&mut code, 3);
    code.local_set(4);
    code.local_get(5)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Result(ValType::I64));
    code.local_get(4).i32_load8_u(arg(0, 0)).i64_extend_i32_u();
    code.else_()
        .local_get(5)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Result(ValType::I64));
    code.local_get(4).i32_load16_u(arg(0, 0)).i64_extend_i32_u();
    code.else_()
        .local_get(4)
        .i32_load(arg(0, 0))
        .i64_extend_i32_u()
        .end()
        .end();
    code.br(1).end();
    code.local_get(0).local_get(1).local_get(2).call(slow_index);
    code.end().end();
    function
}

pub(super) fn emit_write(slow_index: u32) -> Function {
    // params: context, linear address, byte width, value; locals: page, pointer, scratch, version.
    let mut function = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    let mut code = function.instructions();
    code.block(BlockType::Result(ValType::I32))
        .block(BlockType::Empty);
    geometry(&mut code, 4, 2, 2, None);

    // a writable virtual view can still alias executable physical bytes.
    load_field(&mut code, EXECUTABLE_VIEWS);
    code.local_get(4)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(0);
    code.block(BlockType::Empty);
    load_field(&mut code, PRIVATE_SLOTS);
    code.local_tee(6).i32_eqz().br_if(0);
    code.local_get(6);
    page(&mut code);
    code.i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(arg(0, 2))
        .br_if(1);
    code.end();

    load_field(&mut code, TRANSACTION_VERSION);
    code.local_tee(6)
        .i64_load(arg(0, 3))
        .local_tee(7)
        .i64_const(-1)
        .i64_eq()
        .br_if(0);
    code.local_get(7).i64_const(1).i64_add().local_set(7);
    physical_address(&mut code, 4);
    code.local_set(5);
    code.local_get(2)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(5).local_get(3).i32_store8(arg(0, 0));
    code.else_()
        .local_get(2)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    code.local_get(5).local_get(3).i32_store16(arg(0, 0));
    code.else_()
        .local_get(5)
        .local_get(3)
        .i32_store(arg(0, 0))
        .end()
        .end();

    load_field(&mut code, PAGE_VERSIONS);
    code.local_get(4)
        .i32_const(4)
        .i32_shl()
        .i32_add()
        .local_get(7)
        .i64_store(arg(8, 3));
    code.local_get(6).local_get(7).i64_store(arg(0, 3));
    code.i32_const(0).br(1).end();
    code.local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .call(slow_index);
    code.end().end();
    function
}
