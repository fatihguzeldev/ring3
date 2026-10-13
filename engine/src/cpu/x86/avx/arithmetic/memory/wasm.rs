use std::{env, path::PathBuf, process::Command};

#[test]
fn real_numeric_backend_consumes_exact_captured_operands() {
    let backend = env::var_os("RING3_AVX_NUMERIC_WASM")
        .expect("foundation memory proof requires the separately compiled private Rust backend");
    let output = env::var_os("RING3_AVX_PROOF_OUTPUT")
        .expect("foundation memory proof requires a unique retained output directory");
    let source_root = env::var_os("RING3_AVX_NUMERIC_SOURCE_ROOT")
        .expect("the numeric proof requires its matching backend source root");
    let receipt = env::var_os("RING3_AVX_NUMERIC_RECEIPT")
        .expect("the numeric proof requires its matching backend build receipt");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/cpu/x86/avx/arithmetic/memory/fixtures/runtime/run.mjs");
    let result = Command::new(env::var_os("RING3_NODE").unwrap_or_else(|| "node".into()))
        .arg(script)
        .arg(backend)
        .arg(output)
        .arg(source_root)
        .arg(receipt)
        .output()
        .expect("start foundation real Rust Wasm proof");
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8_lossy(&result.stdout));
}
