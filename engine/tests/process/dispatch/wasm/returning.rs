use super::support_wasm as wasm_support;

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Module, TypeSection, ValType,
};

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
    let mut code = CodeSection::new();
    code.function(&body);
    module.section(&code);
    module.finish()
}

#[test]
fn actual_engine_wasm_runs_product_returning_dispatcher() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").ok();
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
        .join("target/p2-returning-dispatcher-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("wrong.wasm"), wrong_signature()).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/process/dispatch/fixtures/p2-returning-dispatcher/runtime/run.mjs",
        &engine,
        &output,
        expected.as_deref(),
    );
    assert!(
        result.status.success(),
        "actual product returning dispatcher Node integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
