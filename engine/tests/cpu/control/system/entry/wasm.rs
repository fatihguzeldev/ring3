use super::support_wasm as wasm_support;
use ring3_engine::process::EngineInstance;
use std::{fs, path::Path, time::SystemTime};

#[test]
fn actual_sysenter_preserves_application_context_and_exports_a_precise_entry() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("select its exact SHA-256");
    let output = root.join("target/p2-system-entry").join(format!(
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
        "engine/tests/cpu/control/system/entry/fixtures/p2-system-entry/runtime/run.mjs",
        Path::new(&engine),
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "system entry integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
