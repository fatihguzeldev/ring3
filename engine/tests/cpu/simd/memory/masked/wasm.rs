use super::support_wasm as wasm_support;

use ring3_engine::{abi::arena::X87_OFFSET, process::EngineInstance};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_maskmov_cached_write_authority_preserves_faults_mmx_and_sparse_ram_policy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .expect("set RING3_ENGINE_WASM to the integrated engine artifact");
    assert!(engine.is_file(), "selected engine artifact must exist");
    let expected_sha =
        std::env::var("RING3_ENGINE_SHA256").expect("set the selected engine SHA-256");
    let output = root
        .join("target/p2-maskmov-authority-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 0x4d41_534b)
        .unwrap()
        .arena()
        .to_vec();
    assert_eq!(initial.len(), 4796);
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x2800_u16.to_le_bytes());
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/memory/masked/fixtures/p2-maskmov-authority/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected_sha),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "MASKMOV cached authority integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
