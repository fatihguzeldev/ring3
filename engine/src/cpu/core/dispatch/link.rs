use crate::cpu::core::{
    COLD_MARK, STOP_MARK, StopReason,
    emitter::{link, support},
};
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, InstructionSink, MemoryType, Module,
    NameMap, NameSection, RefType, TableType, TypeSection, ValType,
};

const STATUS: u32 = 3;
const REMAINING: u32 = 4;
const PREVIOUS_RETIRED: u32 = 5;
const COLD_WORK: u32 = 6;
const COLD_RETIRED: u32 = 7;
const NEXT: u32 = 8;
const LAST: u32 = 9;
const STATE_BASE: u32 = 4;

fn state(local: u32) -> u32 {
    STATE_BASE + link::global(local)
}

fn publish(code: &mut InstructionSink<'_>) {
    code.local_get(0).local_get(LAST).call_indirect(1, 3);
}

fn reject(code: &mut InstructionSink<'_>, canonical: bool) {
    code.if_(BlockType::Empty);
    if canonical {
        code.global_get(STATE_BASE + link::VALID)
            .if_(BlockType::Empty);
        publish(code);
        code.end();
    } else {
        publish(code);
    }
    code.i32_const(StopReason::Infrastructure as i32)
        .return_()
        .end();
}

pub(super) fn emit() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 2], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 3], [ValType::I32]);
    types.ty().function([ValType::I32], []);
    let support_types = support::types(&mut types);
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
    for name in ["functions", "publishers"] {
        imports.import(
            "ring3",
            name,
            EntityType::Table(TableType {
                element_type: RefType::FUNCREF,
                table64: false,
                minimum: 1,
                maximum: Some(4097),
                shared: false,
            }),
        );
    }
    imports.import("ring3", "probe", EntityType::Function(2));
    imports.import("ring3", "cold", EntityType::Function(1));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(2);
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
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 2);
    for (index, name) in ["regions", "coldRuns", "coldWork", "coldRetired"]
        .into_iter()
        .enumerate()
    {
        exports.export(name, ExportKind::Global, index as u32);
    }
    link::define_globals(&mut globals, &mut exports, STATE_BASE);
    support::define(&mut functions, support_types, &mut globals, &mut exports, 3);
    module.section(&functions);
    module.section(&globals);
    module.section(&exports);
    let mut run = Function::new([(7, ValType::I32)]);
    let mut code = run.instructions();
    for index in 0..4 {
        code.i32_const(0).global_set(index);
    }
    for index in [link::MODE, link::VALID, link::CONTEXT] {
        code.i32_const(0).global_set(STATE_BASE + index);
    }
    code.loop_(BlockType::Empty);
    code.local_get(2)
        .i32_eqz()
        .local_get(2)
        .i32_const(4096)
        .i32_gt_u()
        .i32_or();
    reject(&mut code, true);
    for table in 0..2 {
        code.local_get(2).table_size(table).i32_ge_u();
        reject(&mut code, true);
        code.local_get(2).table_get(table).ref_is_null();
        reject(&mut code, true);
    }
    code.local_get(2).local_set(LAST);
    code.global_get(0)
        .i32_const(1)
        .i32_add()
        .global_set(0)
        .local_get(0)
        .local_get(1)
        .local_get(2)
        .call_indirect(0, 0)
        .local_set(STATUS)
        .local_get(STATUS)
        .i32_const(StopReason::NeedCode as i32)
        .i32_ne()
        .if_(BlockType::Empty);
    publish(&mut code);
    code.local_get(STATUS).return_().end();
    code.global_get(STATE_BASE + link::VALID)
        .i32_eqz()
        .global_get(STATE_BASE + link::CONTEXT)
        .local_get(0)
        .i32_ne()
        .i32_or();
    reject(&mut code, false);
    code.global_get(state(12)).local_tee(REMAINING);
    code.local_get(1).i32_gt_u();
    reject(&mut code, false);
    code.local_get(REMAINING).i32_eqz().if_(BlockType::Empty);
    publish(&mut code);
    code.i32_const(StopReason::Budget as i32)
        .return_()
        .end()
        .local_get(REMAINING)
        .local_get(1)
        .i32_eq()
        .if_(BlockType::Empty);
    publish(&mut code);
    code.i32_const(StopReason::NeedCode as i32)
        .return_()
        .end()
        .local_get(REMAINING)
        .local_set(1)
        .global_get(state(13))
        .local_set(PREVIOUS_RETIRED)
        .local_get(0)
        .global_get(state(10))
        .global_get(state(13))
        .call(0)
        .local_set(NEXT)
        .local_get(NEXT)
        .i32_const(COLD_MARK as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    publish(&mut code);
    code.local_get(0)
        .call(1)
        .local_set(NEXT)
        .local_get(0)
        .i32_load(super::super::emitter::mem(44, 2))
        .local_tee(REMAINING)
        .local_get(1)
        .i32_gt_u();
    reject(&mut code, true);
    code.local_get(1)
        .local_get(REMAINING)
        .i32_sub()
        .local_set(COLD_WORK)
        .local_get(0)
        .i32_load(super::super::emitter::mem(40, 2))
        .local_get(PREVIOUS_RETIRED)
        .i32_sub()
        .local_tee(COLD_RETIRED)
        .local_get(COLD_WORK)
        .i32_gt_u();
    reject(&mut code, true);
    code.local_get(NEXT)
        .i32_const(STOP_MARK as i32)
        .i32_and()
        .i32_eqz()
        .local_get(NEXT)
        .i32_const(!(COLD_MARK | STOP_MARK) as i32)
        .i32_and()
        .i32_eqz()
        .i32_eqz()
        .i32_and()
        .local_get(COLD_WORK)
        .i32_eqz()
        .i32_and();
    reject(&mut code, true);
    for (global, local) in [(2, COLD_WORK), (3, COLD_RETIRED)] {
        code.global_get(global)
            .local_get(local)
            .i32_add()
            .global_set(global);
    }
    code.global_get(1)
        .i32_const(1)
        .i32_add()
        .global_set(1)
        .local_get(NEXT)
        .i32_const(COLD_MARK as i32)
        .i32_and()
        .i32_eqz();
    reject(&mut code, true);
    code.local_get(NEXT)
        .i32_const(!(COLD_MARK | STOP_MARK) as i32)
        .i32_and()
        .local_set(2)
        .local_get(NEXT)
        .i32_const(STOP_MARK as i32)
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(2)
        .i32_const(1)
        .i32_sub()
        .i32_const(9)
        .i32_gt_u();
    reject(&mut code, true);
    code.local_get(2).return_().end();
    code.local_get(2).i32_const(4096).i32_gt_u();
    reject(&mut code, true);
    code.else_();
    code.local_get(NEXT).i32_const(4096).i32_gt_u();
    reject(&mut code, false);
    code.local_get(NEXT).i32_eqz().if_(BlockType::Empty);
    publish(&mut code);
    code.i32_const(StopReason::NeedCode as i32)
        .return_()
        .end()
        .local_get(NEXT)
        .local_set(2)
        .end()
        .local_get(REMAINING)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(StopReason::Budget as i32)
        .return_()
        .end()
        .local_get(2)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(StopReason::NeedCode as i32)
        .return_()
        .end()
        .local_get(REMAINING)
        .local_set(1)
        .br(0)
        .end()
        .unreachable()
        .end();
    let mut bodies = CodeSection::new();
    bodies.function(&run);
    support::bodies(&mut bodies, 3);
    module.section(&bodies);
    let mut names = NameSection::new();
    names.module("ring3.dispatch");
    let mut functions = NameMap::new();
    functions.append(0, "guest.probe");
    functions.append(1, "guest.cold");
    functions.append(2, "guest.chain");
    support::names(&mut functions, 3);
    names.functions(&functions);
    module.section(&names);
    module.finish()
}
