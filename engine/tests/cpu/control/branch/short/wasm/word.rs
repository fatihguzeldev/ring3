use super::support_wasm as wasm_support;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_short_word_targets_and_count_widths_in_bound_modes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .expect("select the integrated engine Wasm");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the engine SHA256");
    assert!(engine.is_file());
    let output = root.join("target/p2-short-word-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/branch/short/fixtures/p2-short-word/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    assert!(
        result.status.success(),
        "short-word fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
