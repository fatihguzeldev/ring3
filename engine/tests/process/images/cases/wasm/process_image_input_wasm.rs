use super::support_wasm as wasm_support;

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_wasm_commits_ordered_large_pe_input_and_runs_linked_terminal_guest() {
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
    let output = root.join("target/p2-image-input-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/process/images/fixtures/p2-image-input/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "actual ordered large PE32 input integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
