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
    types.ty().function([ValType::I32; 3], []);
    types.ty().function([ValType::I32], [ValType::I32]);
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
    functions
        .function(1)
        .function(2)
        .function(2)
        .function(3)
        .function(3);
    module.section(&functions);
    let mut globals = GlobalSection::new();
    for _ in 0..5 {
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
    for (name, index) in [
        ("guard", 0),
        ("configure", 1),
        ("read8", 2),
        ("read16", 3),
        ("calls", 4),
        ("address", 5),
    ] {
        exports.export(name, ExportKind::Func, index);
    }
    module.section(&exports);
    let mut code = CodeSection::new();
    let mut configure = Function::new([]);
    let mut body = configure.instructions();
    for index in 0..3 {
        body.local_get(index).global_set(index);
    }
    body.i32_const(0)
        .global_set(3)
        .i32_const(0)
        .global_set(4)
        .end();
    code.function(&configure);
    for _ in 0..2 {
        let mut helper = Function::new([]);
        let mut body = helper.instructions();
        body.global_get(3)
            .i32_const(1)
            .i32_add()
            .global_set(3)
            .local_get(0)
            .global_set(4);
        body.global_get(2)
            .i32_eqz()
            .global_get(2)
            .i32_const(11)
            .i32_eq()
            .i32_or()
            .if_(BlockType::Empty)
            .global_get(0)
            .global_get(1)
            .i32_const(40)
            .memory_copy(0, 0)
            .end()
            .global_get(2)
            .end();
        code.function(&helper);
    }
    for global in [3, 4] {
        let mut accessor = Function::new([]);
        accessor.instructions().global_get(global).end();
        code.function(&accessor);
    }
    module.section(&code);
    module.finish()
}

#[test]
fn actual_engine_wasm_executes_exact_width_memory_extensions() {
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
    let output = root
        .join("target/p2-memory-extensions-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let injector = output.join("helper-injector.wasm");
    fs::write(&injector, helper_injector()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-extensions/run.mjs"))
        .arg(&engine)
        .arg(&injector)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine memory-extension Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
