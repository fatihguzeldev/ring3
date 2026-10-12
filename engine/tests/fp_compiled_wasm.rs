use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn original_compiled_fp_functions_preserve_cdecl_state_and_retry_checked_faults() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let output = root.join("target/p2-fp-compiled-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 7).unwrap().arena(),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-fp-compiled/run.mjs"))
        .arg(engine)
        .arg(output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "compiled FP integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
