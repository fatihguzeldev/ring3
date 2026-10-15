use super::support_wasm as wasm_support;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_scalar_counts_and_movbe_preserve_flags_widths_and_fault_atomicity() {
    run_fixture("run.mjs");
}

#[test]
fn actual_crc32_preserves_flags_aliases_source_widths_and_fault_atomicity() {
    run_fixture("crc32.mjs");
}

fn run_fixture(script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root.join("target/p2-scalar-bits-fixtures").join(format!(
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
        &format!(
            "engine/tests/cpu/bits_flags/integration/fixtures/p2-scalar-bits/runtime/{script}"
        ),
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "scalar-bit {script} integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
