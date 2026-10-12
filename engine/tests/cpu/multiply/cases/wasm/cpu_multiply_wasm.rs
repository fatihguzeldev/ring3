use super::support_wasm as wasm_support;

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_wasm_executes_signed_multiply_with_precise_overflow_flags_and_resume() {
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
    let output = root.join("target/p2-multiply-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/multiply/fixtures/p2-multiply/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "actual signed-multiply integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
