use std::{env, path::PathBuf, process::Command};

#[test]
fn shared_numeric_backend_preserves_complete_staged_cpu_state() {
    let backend = env::var_os("RING3_AVX_NUMERIC_WASM").expect(
        "the compound Wasm proof requires the separately compiled private Rust numeric backend",
    );
    let output = env::var_os("RING3_AVX_PROOF_OUTPUT")
        .expect("the compound Wasm proof requires a unique retained output directory");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/cpu/x86/avx/compound/fixtures/runtime/run.mjs");
    let result = Command::new(env::var_os("RING3_NODE").unwrap_or_else(|| "node".into()))
        .arg(script)
        .arg(backend)
        .arg(output)
        .output()
        .expect("start the compound numeric backend proof");
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    print!("{}", String::from_utf8_lossy(&result.stdout));
}
