use super::support_wasm as wasm_support;
use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_system_register_bank_uses_supplied_images_and_atomic_query_stores() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .expect("set RING3_ENGINE_WASM to the integrated engine artifact");
    assert!(engine.is_file(), "selected engine artifact must exist");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select its explicit SHA-256");
    let output = root
        .join("target/p2-system-registers-fixtures")
        .join(format!(
            "code-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 0x53595247).unwrap().arena().to_vec();
    assert_eq!(initial.len(), 4796);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/control/system_registers/fixtures/p2-system-registers/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "system-register query bank integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
