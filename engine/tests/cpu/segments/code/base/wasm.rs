use super::support_wasm as wasm_support;
use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_code_bases_partition_decode_capture_routing_and_repair() {
    run("run.mjs", "code");
}

#[test]
fn bulk_pop_staging_stops_before_consuming_a_changed_code_context() {
    run("stack.mjs", "stack");
}

#[test]
fn segment_query_staging_stops_before_consuming_a_changed_code_context() {
    run("queries.mjs", "queries");
}

#[test]
fn numeric_staging_stops_before_consuming_a_changed_code_context() {
    run("numeric.mjs", "numeric");
}

#[test]
fn gather_staging_stops_before_publishing_a_changed_code_context() {
    run("gather.mjs", "gather");
}

fn run(script: &str, label: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .expect("set RING3_ENGINE_WASM to the integrated engine artifact");
    assert!(engine.is_file(), "selected engine artifact must exist");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select its explicit SHA-256");
    let output = root.join("target/p2-cs-base-fixtures").join(format!(
        "{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 0x43534241).unwrap().arena().to_vec();
    assert_eq!(initial.len(), 4796);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        &format!("engine/tests/cpu/segments/code/base/fixtures/p2-cs-base/runtime/{script}"),
        &engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "CS base integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
