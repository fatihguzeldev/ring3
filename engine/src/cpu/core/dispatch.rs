use super::{COLD_MARK, STOP_MARK, StopReason};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, MemArg, MemoryType, Module, NameMap,
    NameSection, RefType, TableType, TypeSection, ValType,
};

const STATUS: u32 = 3;
const REMAINING: u32 = 4;
const PREVIOUS_RETIRED: u32 = 5;
const COLD_WORK: u32 = 6;
const COLD_RETIRED: u32 = 7;

pub fn emit_dispatch() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 2], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 3], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "ring3",
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
        "ring3",
        "functions",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 1,
            maximum: Some(4097),
            shared: false,
        }),
    );
    imports.import("ring3", "next", EntityType::Function(1));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(2);
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for _ in 0..4 {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
                shared: false,
            },
            &ConstExpr::i32_const(0),
        );
    }
    module.section(&globals);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 1);
    exports.export("regions", ExportKind::Global, 0);
    exports.export("coldRuns", ExportKind::Global, 1);
    exports.export("coldWork", ExportKind::Global, 2);
    exports.export("coldRetired", ExportKind::Global, 3);
    module.section(&exports);
    let mut run = Function::new([(5, ValType::I32)]);
    let mut code = run.instructions();
    for index in 0..4 {
        code.i32_const(0).global_set(index);
    }
    code.loop_(BlockType::Empty);
    code.local_get(2)
        .i32_eqz()
        .local_get(2)
        .i32_const(4096)
        .i32_gt_u()
        .i32_or()
        .local_get(2)
        .table_size(0)
        .i32_ge_u()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    code.local_get(2)
        .table_get(0)
        .ref_is_null()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    code.global_get(0).i32_const(1).i32_add().global_set(0);
    code.local_get(0)
        .local_get(1)
        .local_get(2)
        .call_indirect(0, 0)
        .local_set(STATUS);
    code.local_get(STATUS)
        .i32_const(StopReason::NeedCode as i32)
        .i32_ne()
        .if_(BlockType::Empty)
        .local_get(STATUS)
        .return_()
        .end();
    code.local_get(0)
        .i32_load(MemArg {
            offset: 44,
            align: 2,
            memory_index: 0,
        })
        .local_set(REMAINING);
    code.local_get(REMAINING)
        .local_get(1)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    code.local_get(REMAINING)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Budget as i32)
        .return_()
        .end();
    code.local_get(REMAINING)
        .local_get(1)
        .i32_eq()
        .if_(BlockType::Empty)
        .i32_const(StopReason::NeedCode as i32)
        .return_()
        .end();
    code.local_get(REMAINING).local_set(1);
    code.local_get(0)
        .i32_load(MemArg {
            offset: 40,
            align: 2,
            memory_index: 0,
        })
        .local_set(PREVIOUS_RETIRED);
    code.local_get(0).call(0).local_set(STATUS);
    code.local_get(0)
        .i32_load(MemArg {
            offset: 44,
            align: 2,
            memory_index: 0,
        })
        .local_tee(REMAINING)
        .local_get(1)
        .i32_gt_u()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    code.local_get(1)
        .local_get(REMAINING)
        .i32_sub()
        .local_set(COLD_WORK);
    code.local_get(0)
        .i32_load(MemArg {
            offset: 40,
            align: 2,
            memory_index: 0,
        })
        .local_get(PREVIOUS_RETIRED)
        .i32_sub()
        .local_set(COLD_RETIRED);
    code.local_get(STATUS)
        .i32_const(COLD_MARK as i32)
        .i32_and()
        .if_(BlockType::Empty);
    code.local_get(STATUS)
        .i32_const(!(COLD_MARK | STOP_MARK) as i32)
        .i32_and()
        .local_set(2);
    code.local_get(STATUS)
        .i32_const(STOP_MARK as i32)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .local_get(2)
        .i32_const(1)
        .i32_sub()
        .i32_const(9)
        .i32_gt_u()
        .else_()
        .local_get(2)
        .i32_const(4096)
        .i32_gt_u()
        .local_get(2)
        .i32_eqz()
        .local_get(COLD_WORK)
        .i32_or()
        .i32_eqz()
        .i32_or()
        .end();
    code.local_get(COLD_RETIRED)
        .local_get(COLD_WORK)
        .i32_gt_u()
        .i32_or()
        .global_get(1)
        .global_get(2)
        .local_get(COLD_WORK)
        .i32_add()
        .i32_gt_u()
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    for (global, local) in [(2, COLD_WORK), (3, COLD_RETIRED)] {
        code.global_get(global)
            .local_get(local)
            .i32_add()
            .global_set(global);
    }
    code.global_get(1).i32_const(1).i32_add().global_set(1);
    code.local_get(STATUS)
        .i32_const(STOP_MARK as i32)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(2)
        .return_()
        .end();
    code.else_();
    code.local_get(STATUS)
        .i32_const(4096)
        .i32_gt_u()
        .local_get(COLD_WORK)
        .i32_or()
        .local_get(COLD_RETIRED)
        .i32_or()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
    code.local_get(STATUS).local_set(2).end();
    code.local_get(REMAINING)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Budget as i32)
        .return_()
        .end();
    code.local_get(REMAINING).local_set(1);
    code.local_get(2)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(StopReason::NeedCode as i32)
        .return_()
        .end();
    code.br(0).end().unreachable().end();
    let mut bodies = CodeSection::new();
    bodies.function(&run);
    module.section(&bodies);
    let mut names = NameSection::new();
    names.module("ring3.dispatch");
    let mut functions = NameMap::new();
    functions.append(0, "guest.next");
    functions.append(1, "guest.chain");
    names.functions(&functions);
    module.section(&names);
    module.finish()
}

#[cfg(test)]
#[path = "dispatch/tests.rs"]
mod tests;
