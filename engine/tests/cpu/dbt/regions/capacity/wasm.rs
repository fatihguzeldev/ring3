use super::support_wasm;
use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
#[test]
fn actual_automatic_capacity_prefixes_resume_at_exact_fallthrough_without_weakening_whole_units() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin selected engine SHA256");
    let output = root.join("target/p2-module-capacity").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 0x43415041).unwrap().arena(),
    )
    .unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/cpu/dbt/regions/capacity/fixtures/p2-module-capacity/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
