use super::support_wasm;

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_x87_success_packets_reject_illegal_integer_flags_before_publication() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::path::PathBuf::from(
        std::env::var_os("RING3_ENGINE_WASM")
            .expect("provide the validated engine Wasm with RING3_ENGINE_WASM"),
    );
    assert!(engine.is_file(), "the selected engine Wasm must exist");
    let expected = std::env::var("RING3_ENGINE_SHA256")
        .expect("provide the selected engine SHA-256 with RING3_ENGINE_SHA256");
    let output = root.join("target/p2-x87-helper-protocol").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/cpu/x87/protocol/fixtures/p2-x87-helper-protocol/runtime/run.mjs",
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
