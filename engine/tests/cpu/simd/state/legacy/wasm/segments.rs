use super::support_wasm as wasm_support;

use ring3_engine::abi::x86::decode_exit;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_fx_images_enforce_cached_full_spans_without_changing_state_on_fault() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build integrated engine wasm first");
    let output = root
        .join("target/p2-fx-segment-access-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/state/legacy/fixtures/p2-fx-segment-access/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "FX cached span actual failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let exits = fs::read(output.join("exit-records.bin")).unwrap();
    assert_eq!(exits.len(), 148 * 40);
    for (index, record) in exits.chunks_exact(40).enumerate() {
        decode_exit(record).unwrap_or_else(|error| panic!("actual exit{index}: {error:?}"));
    }
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
