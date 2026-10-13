use super::support_wasm;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_unavailable_instructions_fault_only_after_complete_fetch() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .expect("select the integrated engine Wasm");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the engine SHA256");
    assert!(engine.is_file());
    let output = root.join("target/p2-availability").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/cpu/control/exceptions/availability/fixtures/p2-availability/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
