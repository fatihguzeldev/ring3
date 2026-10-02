use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    InstructionSink, MemArg, MemoryType, Module, RefType, TableType, TypeSection, ValType,
};

const TOTAL: u32 = 4;
const STATUS: u32 = 5;
const SLOT: u32 = 6;
const CHILD_RETIRED: u32 = 7;
const REASON: u32 = 8;

fn memarg(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

fn load(code: &mut InstructionSink<'_>, pointer: u32, offset: u64) {
    code.local_get(pointer).i32_load(memarg(offset));
}

fn synthetic(code: &mut InstructionSink<'_>, reason: i32) {
    for (offset, value) in [
        (0, i32::from_le_bytes(*b"R3EX")),
        (4, 0x10003),
        (8, 40),
        (12, 0),
        (16, reason),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 0),
    ] {
        code.local_get(1).i32_const(value).i32_store(memarg(offset));
    }
    code.local_get(1)
        .local_get(TOTAL)
        .i32_store(memarg(20))
        .i32_const(0)
        .return_();
}

fn host_failure(code: &mut InstructionSink<'_>) {
    code.local_get(TOTAL)
        .if_(BlockType::Empty)
        .local_get(1)
        .local_get(TOTAL)
        .i32_store(memarg(20))
        .end()
        .local_get(STATUS)
        .return_();
}

fn dispatcher(key: u64) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 3], [ValType::I32]);
    module.section(&types);
    let mut imports = wasm_encoder::ImportSection::new();
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
        "table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 8,
            maximum: Some(8),
            shared: false,
        }),
    );
    imports.import("ring3", "find_installed_resident", EntityType::Function(1));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 1);
    module.section(&exports);
    let mut run = Function::new([(5, ValType::I32)]);
    let mut code = run.instructions();
    code.i32_const(0).local_set(TOTAL).loop_(BlockType::Empty);
    load(&mut code, 3, 0);
    code.if_(BlockType::Empty);
    synthetic(&mut code, 2);
    code.end().local_get(2).i32_eqz().if_(BlockType::Empty);
    synthetic(&mut code, 1);
    code.end()
        .i32_const(key as u32 as i32)
        .i32_const((key >> 32) as u32 as i32);
    load(&mut code, 0, 48);
    code.call(0)
        .local_tee(STATUS)
        .i32_const(17)
        .i32_eq()
        .if_(BlockType::Empty);
    synthetic(&mut code, 3);
    code.end().local_get(STATUS).if_(BlockType::Empty);
    host_failure(&mut code);
    code.end();
    load(&mut code, 0, 164);
    code.local_tee(SLOT)
        .table_size(0)
        .i32_ge_u()
        .if_(BlockType::Empty);
    synthetic(&mut code, 3);
    code.end()
        .local_get(SLOT)
        .table_get(0)
        .ref_is_null()
        .if_(BlockType::Empty);
    synthetic(&mut code, 3);
    code.end()
        .local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .local_get(SLOT)
        .call_indirect(0, 0)
        .local_tee(STATUS)
        .if_(BlockType::Empty);
    host_failure(&mut code);
    code.end();
    load(&mut code, 1, 20);
    code.local_tee(CHILD_RETIRED)
        .local_get(TOTAL)
        .i32_add()
        .local_set(TOTAL)
        .local_get(2)
        .local_get(CHILD_RETIRED)
        .i32_sub()
        .local_set(2);
    load(&mut code, 1, 16);
    code.local_set(REASON)
        .local_get(REASON)
        .i32_const(3)
        .i32_ne()
        .local_get(CHILD_RETIRED)
        .i32_eqz()
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(1)
        .local_get(TOTAL)
        .i32_store(memarg(20))
        .i32_const(0)
        .return_()
        .end()
        .br(0)
        .end()
        .i32_const(0)
        .end();
    let mut bodies = CodeSection::new();
    bodies.function(&run);
    module.section(&bodies);
    module.finish()
}

fn wrong_signature() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    module.section(&types);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("wrong", ExportKind::Func, 0);
    module.section(&exports);
    let mut body = Function::new([]);
    body.instructions().i32_const(7).end();
    let mut bodies = CodeSection::new();
    bodies.function(&body);
    module.section(&bodies);
    module.finish()
}

#[test]
fn actual_engine_wasm_acknowledges_resident_installations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root
        .join("target/p2-resident-installation-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    for ordinal in 1..=3 {
        fs::write(
            output.join(format!("dispatcher-{ordinal}.wasm")),
            dispatcher(0x91345678e0000000 + ordinal),
        )
        .unwrap();
    }
    fs::write(output.join("wrong.wasm"), wrong_signature()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-resident-installation/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine installation Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
