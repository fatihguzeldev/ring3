use super::support_wasm as wasm_support;

use ring3_engine::process::EngineInstance;
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_stack_frames_round_trip_nested_frames_and_precise_failures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    let output = root.join("target/p2-stack-frames-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    initial[4252..4254].copy_from_slice(&0x027fu16.to_le_bytes());
    initial[4380..4384].copy_from_slice(&0x5f80u32.to_le_bytes());
    initial[4396..4524].fill(0xa5);
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/stack/frame/fixtures/p2-stack-frames/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "stack frame integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
