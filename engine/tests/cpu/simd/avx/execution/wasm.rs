use super::support_wasm as wasm_support;
use ring3_engine::abi::arena::ARENA_SIZE;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_avx_execution_preserves_upper_state_exact_spans_and_mixed_locals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(
        engine.is_file(),
        "build the coherent AVX verification engine first"
    );
    assert_eq!(ARENA_SIZE, 4796);
    let output = root.join("target/p2-avx-execution").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/avx/execution/fixtures/p2-avx-execution/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "AVX actual integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
