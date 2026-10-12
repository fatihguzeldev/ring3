use super::support_wasm as wasm_support;

use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_integer_completion_preserves_flags_aliases_faults_and_currentness() {
    run_fixture("run.mjs");
}

#[test]
fn actual_cache_flush_and_rdtscp_preserve_faults_currentness_and_ordering() {
    run_fixture("cache_timestamp.mjs");
}

fn run_fixture(script: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build integrated engine wasm first");
    let output = root
        .join("target/p2-integer-completion-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(initial.len(), ARENA_SIZE);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        &format!("engine/tests/cpu/profile/fixtures/p2-integer-completion/runtime/{script}"),
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "integer {script} actual failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
