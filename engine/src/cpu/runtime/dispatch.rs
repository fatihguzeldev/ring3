use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemArg, MemoryType, Module, RefType, TableType, TypeSection, ValType,
};

fn mem(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

pub fn emit_dispatch() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import(
        "env",
        "functions",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 1,
            maximum: None,
            shared: false,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 0);
    module.section(&exports);
    let mut function = Function::new([(7, ValType::I32)]);
    let c = &mut function.instructions();
    c.local_get(0)
        .i32_load(mem(0))
        .i32_const(1)
        .i32_ne()
        .local_get(0)
        .i32_load(mem(4))
        .i32_const(160)
        .i32_ne()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(8)
        .return_()
        .end();
    c.loop_(BlockType::Empty);
    c.local_get(0)
        .i32_load(mem(48))
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_const(2)
        .i32_store(mem(52))
        .i32_const(2)
        .return_()
        .end();
    c.local_get(0).i32_load(mem(40)).local_set(1);
    c.local_get(0).i32_load(mem(144)).local_set(2);
    c.local_get(0).i32_load(mem(148)).local_set(3);
    c.local_get(1)
        .i32_const(1)
        .i32_shr_u()
        .local_get(1)
        .i32_const(12)
        .i32_shr_u()
        .i32_xor()
        .local_get(3)
        .i32_and()
        .local_set(4);
    c.i32_const(0).local_set(5);
    c.loop_(BlockType::Empty);
    c.local_get(2)
        .local_get(4)
        .i32_const(3)
        .i32_shl()
        .i32_add()
        .local_tee(6)
        .i32_load(mem(4))
        .local_set(7);
    c.local_get(7)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_const(1)
        .i32_store(mem(52))
        .i32_const(1)
        .return_()
        .end();
    c.local_get(7)
        .i32_const(-1)
        .i32_ne()
        .local_get(6)
        .i32_load(mem(0))
        .local_get(1)
        .i32_eq()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .local_get(7)
        .call_indirect(0, 0)
        .local_tee(7)
        .if_(BlockType::Empty)
        .local_get(7)
        .return_()
        .end()
        .br(2)
        .end();
    c.local_get(5)
        .i32_const(1)
        .i32_add()
        .local_tee(5)
        .local_get(3)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_const(1)
        .i32_store(mem(52))
        .i32_const(1)
        .return_()
        .end();
    c.local_get(4)
        .i32_const(1)
        .i32_add()
        .local_get(3)
        .i32_and()
        .local_set(4)
        .br(0);
    c.end().end().unreachable().end();
    let mut code = CodeSection::new();
    code.function(&function);
    module.section(&code);
    module.finish()
}
