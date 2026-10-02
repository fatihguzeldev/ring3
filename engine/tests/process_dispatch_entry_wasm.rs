use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use wasm_encoder::{
    BlockType, CodeSection, EntityType, ExportKind, ExportSection, Function, FunctionSection,
    ImportSection, MemoryType, Module, RefType, TableType, TypeSection, ValType,
};

fn entry_probe(key: u64) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function([ValType::I32; 5], [ValType::I32]);
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
        "table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 8,
            maximum: Some(8),
            shared: false,
        }),
    );
    imports.import("ring3", "guard_dispatch_entry", EntityType::Function(1));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 1);
    module.section(&exports);
    let mut run = Function::new([(1, ValType::I32)]);
    run.instructions()
        .i32_const(key as u32 as i32)
        .i32_const((key >> 32) as u32 as i32)
        .local_get(0)
        .local_get(1)
        .local_get(3)
        .call(0)
        .local_tee(4)
        .if_(BlockType::Empty)
        .local_get(4)
        .return_()
        .end()
        .local_get(0)
        .local_get(1)
        .local_get(2)
        .local_get(3)
        .i32_const(0)
        .call_indirect(0, 0)
        .end();
    let mut bodies = CodeSection::new();
    bodies.function(&run);
    module.section(&bodies);
    module.finish()
}

#[test]
fn actual_engine_wasm_guards_dispatch_entry_before_child_execution() {
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
    let output = root.join("target/p2-dispatch-entry-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let key = 0xa1345678c0000001;
    fs::write(output.join("probe.wasm"), entry_probe(key)).unwrap();
    fs::write(
        output.join("foreign-probe.wasm"),
        entry_probe(key ^ (1 << 32)),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-dispatch-entry/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual engine dispatch-entry Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
