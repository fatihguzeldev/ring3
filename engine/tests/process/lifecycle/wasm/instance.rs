use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    MemoryType, Module, TypeSection, ValType,
};

fn helper_probe() -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 6], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    types.ty().function([ValType::I32; 2], [ValType::I32]);
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
    imports.import("ring3", "read32", EntityType::Function(1));
    imports.import("ring3", "write32", EntityType::Function(2));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0).function(1).function(2);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("guard", ExportKind::Func, 3);
    exports.export("read32", ExportKind::Func, 4);
    exports.export("write32", ExportKind::Func, 5);
    module.section(&exports);
    let mut code = CodeSection::new();
    for (parameters, import) in [(6, 0), (1, 1), (2, 2)] {
        let mut function = Function::new([]);
        let mut body = function.instructions();
        for index in 0..parameters {
            body.local_get(index);
        }
        body.call(import).end();
        code.function(&function);
    }
    module.section(&code);
    module.finish()
}

#[test]
fn actual_engine_wasm_owns_arena_helpers_and_guarded_generated_modules() {
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
    let output = root.join("target/p2-engine-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let probe = output.join("helper-probe.wasm");
    fs::write(&probe, helper_probe()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/src/abi/wasm/tests/run.mjs"))
        .arg(&engine)
        .arg(&probe)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
