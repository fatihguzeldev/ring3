use super::support_wasm;
use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_documented_legacy_nop_aliases_keep_full_fetch_and_no_operand_effects() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the selected engine SHA256");
    assert!(engine.is_file());
    let output = root.join("target/p2-legacy-nop").join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let initial = EngineInstance::new(1, 0x4e4f5041).unwrap().arena().to_vec();
    assert_eq!(initial.len(), 4796);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/cpu/control/hints/reserved/fixtures/p2-legacy-nop/runtime/run.mjs",
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
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
