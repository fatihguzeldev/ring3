use super::support_wasm as wasm_support;
use ring3_engine::process::EngineInstance;
use std::{fs, path::Path, time::SystemTime};

#[test]
fn actual_timestamp_clock_preserves_time_and_unblocks_a_guest_polling_loop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM").expect("select the integrated clock engine");
    let engine = Path::new(&engine);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select its explicit SHA-256");
    let output = root.join("target/cpu-clock").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 7).unwrap().arena(),
    )
    .unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/profile/environment/clock/fixtures/p2-cpu-clock/runtime/run.mjs",
        engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "CPU clock actual failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
