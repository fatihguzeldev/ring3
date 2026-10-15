use super::support_wasm as wasm_support;

use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_original_sse2_pe_uses_and_releases_provider_allocated_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root.join("target/p2-fp-alloc-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(8, 0xf579_0000_0000_0001).unwrap();
    assert_eq!(initial.arena().len(), ARENA_SIZE);
    fs::write(output.join("initial-arena.bin"), initial.arena()).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/process/images/startup/imports/fixtures/p2-c-imports/fp-alloc/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node-stdout.txt"), &result.stdout).unwrap();
    fs::write(output.join("node-stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "original allocated-output PE integration failed; artifacts in {}:\n{}\n{}",
        output.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
