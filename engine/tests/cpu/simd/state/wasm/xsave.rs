use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::{arena::ARENA_SIZE, x86::decode_exit},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_standard_xstate_preserves_selected_components_faults_and_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build integrated engine wasm first");
    let output = root.join("target/p2-xstate-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(initial.len(), ARENA_SIZE);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/state/fixtures/p2-xstate/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "xstate actual failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let exits = fs::read(output.join("exit-records.bin")).unwrap();
    assert!(!exits.is_empty());
    assert_eq!(exits.len() % 40, 0);
    for (index, record) in exits.chunks_exact(40).enumerate() {
        decode_exit(record).unwrap_or_else(|error| panic!("actual exit{index}: {error:?}"));
    }
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
