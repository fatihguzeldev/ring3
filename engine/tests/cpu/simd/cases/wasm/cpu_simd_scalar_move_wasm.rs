use super::support_wasm as wasm_support;

use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_scalar_moves_preserve_bits_checked_memory_and_currentness() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root
        .join("target/p2-simd-scalar-move-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 7).unwrap().arena(),
    )
    .unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/fixtures/p2-simd-scalar-move/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "scalar move integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
