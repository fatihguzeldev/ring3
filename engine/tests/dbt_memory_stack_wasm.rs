use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, MemoryType, Module, TypeSection,
    ValType,
};

fn helper_injector() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 6], [ValType::I32]);
    types.ty().function([ValType::I32; 5], []);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 2], [ValType::I32]);
    types.ty().function([], [ValType::I32]);
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
    imports.import("ring3", "guard", EntityType::Function(0));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(1).function(2).function(3);
    for _ in 0..6 {
        functions.function(4);
    }
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for _ in 0..11 {
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
    exports.export("guard", ExportKind::Func, 0);
    exports.export("configure", ExportKind::Func, 1);
    exports.export("read32", ExportKind::Func, 2);
    exports.export("store32", ExportKind::Func, 3);
    for (index, name) in [
        "read_calls",
        "store_calls",
        "order",
        "store_value",
        "store_address",
        "read_address",
    ]
    .iter()
    .enumerate()
    {
        exports.export(name, ExportKind::Func, index as u32 + 4);
    }
    module.section(&exports);
    let mut code = CodeSection::new();
    let mut configure = Function::new([]);
    let mut body = configure.instructions();
    for index in 0..5 {
        body.local_get(index).global_set(index);
    }
    for index in 5..11 {
        body.i32_const(0).global_set(index);
    }
    body.end();
    code.function(&configure);
    for store in [false, true] {
        let mut helper = Function::new([]);
        let mut body = helper.instructions();
        let count = if store { 6 } else { 5 };
        let status = if store { 4 } else { 3 };
        let record = if store { 2 } else { 1 };
        body.global_get(count)
            .i32_const(1)
            .i32_add()
            .global_set(count);
        body.global_get(7)
            .i32_const(10)
            .i32_mul()
            .i32_const(if store { 2 } else { 1 })
            .i32_add()
            .global_set(7);
        body.local_get(0).global_set(if store { 9 } else { 10 });
        if store {
            body.local_get(1).global_set(8);
        }
        body.global_get(status)
            .i32_eqz()
            .global_get(status)
            .i32_const(11)
            .i32_eq()
            .i32_or()
            .if_(BlockType::Empty)
            .global_get(0)
            .global_get(record)
            .i32_const(40)
            .memory_copy(0, 0)
            .end()
            .global_get(status)
            .end();
        code.function(&helper);
    }
    for index in 5..11 {
        let mut getter = Function::new([]);
        getter.instructions().global_get(index).end();
        code.function(&getter);
    }
    module.section(&code);
    module.finish()
}

#[test]
fn actual_engine_wasm_executes_ordered_guest_memory_stack_operands() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib before this integration test: {}",
        engine.display()
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-memory-stack-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let injector = output.join("helper-injector.wasm");
    fs::write(&injector, helper_injector()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-stack/run.mjs"))
        .arg(&engine)
        .arg(&injector)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine memory stack Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
