use super::support_wasm as wasm_support;
use ring3_engine::abi::x86::decode_exit;
use std::{fs, path::Path, time::{SystemTime, UNIX_EPOCH}};

#[test]
fn actual_bulk_stack_checks_cached_ss_before_staged_memory_publication() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file());
    let output = root.join("target/p2-bulk-segments-fixtures").join(format!(
        "wasm-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(root,
        "engine/tests/cpu/stack/bulk/segments/fixtures/p2-bulk-segments/runtime/run.mjs", &engine, &output, None);
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(result.status.success(), "cached SS proof failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    let exits = fs::read(output.join("exit-records.bin")).unwrap();
    assert_eq!(exits.len(), 176 * 40);
    for (index, bytes) in exits.chunks_exact(40).enumerate() {
        decode_exit(bytes).unwrap_or_else(|error| panic!("exit{index}: {error:?}"));
    }
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
