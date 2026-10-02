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
const PC: u32 = 5;
const SLOT: u32 = 6;
const STATUS: u32 = 7;
const CHILD_RETIRED: u32 = 8;
const REASON: u32 = 9;

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

fn finish(code: &mut InstructionSink<'_>, reason: Option<i32>) {
    for (offset, value) in [
        (0, i32::from_le_bytes(*b"R3EX")),
        (4, 0x10001),
        (8, 40),
        (12, 0),
        (24, 0),
        (28, 0),
        (32, 0),
        (36, 0),
    ] {
        code.local_get(1).i32_const(value).i32_store(memarg(offset));
    }
    code.local_get(1);
    if let Some(reason) = reason {
        code.i32_const(reason);
    } else {
        code.local_get(REASON);
    }
    code.i32_store(memarg(16))
        .local_get(1)
        .local_get(TOTAL)
        .i32_store(memarg(20))
        .i32_const(0)
        .return_();
}

fn exit_if(code: &mut InstructionSink<'_>, reason: i32) {
    code.if_(BlockType::Empty);
    finish(code, Some(reason));
    code.end();
}

// fixed arena buffers and reserved transfer tail are test-only dispatcher controls.
fn dispatcher() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
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
            minimum: 2,
            maximum: Some(2),
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
    let mut run = Function::new([(6, ValType::I32)]);
    let mut code = run.instructions();
    code.local_get(0)
        .i32_const(0)
        .i32_store(memarg(4228))
        .loop_(BlockType::Empty);
    load(&mut code, 3, 0);
    exit_if(&mut code, 2);
    code.local_get(2).i32_eqz();
    exit_if(&mut code, 1);
    load(&mut code, 0, 48);
    code.local_set(PC).i32_const(-1).local_set(SLOT);
    for (pc, slot) in [(0x1000, 0), (0x1001, 0), (0x2000, 1), (0x2001, 1)] {
        code.local_get(PC)
            .i32_const(pc)
            .i32_eq()
            .if_(BlockType::Empty)
            .i32_const(slot)
            .local_set(SLOT)
            .end();
    }
    code.local_get(SLOT).i32_const(-1).i32_eq();
    exit_if(&mut code, 3);
    code.local_get(0)
        .i32_load(memarg(4224))
        .i32_const(1)
        .local_get(SLOT)
        .i32_shl()
        .i32_and()
        .i32_eqz();
    exit_if(&mut code, 3);
    code.local_get(SLOT).table_size(0).i32_ge_u();
    exit_if(&mut code, 3);
    code.local_get(SLOT).table_get(0).ref_is_null();
    exit_if(&mut code, 3);
    code.local_get(0)
        .local_get(0)
        .i32_load(memarg(4228))
        .i32_const(1)
        .i32_add()
        .i32_store(memarg(4228));
    code.local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .local_get(SLOT)
        .call_indirect(0, 0)
        .local_tee(STATUS)
        .if_(BlockType::Empty)
        .local_get(STATUS)
        .return_()
        .end();
    load(&mut code, 1, 20);
    code.local_set(CHILD_RETIRED)
        .local_get(TOTAL)
        .local_get(CHILD_RETIRED)
        .i32_add()
        .local_set(TOTAL)
        .local_get(2)
        .local_get(CHILD_RETIRED)
        .i32_sub()
        .local_set(2);
    load(&mut code, 1, 16);
    code.local_tee(REASON)
        .i32_const(3)
        .i32_ne()
        .if_(BlockType::Empty);
    finish(&mut code, None);
    code.end().local_get(CHILD_RETIRED).i32_eqz();
    exit_if(&mut code, 3);
    code.br(0).end().i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&run);
    module.section(&bodies);
    module.finish()
}

#[test]
fn actual_engine_wasm_guards_process_resident_units() {
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
        .join("target/p2-resident-process-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("dispatcher.wasm"), dispatcher()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-resident-process/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine resident Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
