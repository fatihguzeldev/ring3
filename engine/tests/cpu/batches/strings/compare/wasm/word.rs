use super::support_wasm as wasm_support;

use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_retains_word_move_compare_scan_and_condition_consumers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = crate::support_wasm::selected_engine(root);
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let parent = root.join("target/p2-cpu-parallel-word-move-compare-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&output).unwrap();
    fs::write(
        output.join("word-move-compare.x86"),
        [
            0x66, 0xa5, 0x66, 0xa7, 0x0f, 0x90, 0xc1, 0x66, 0xaf, 0x0f, 0x92, 0xc2, 0xb8, 0x00,
            0x80, 0x3c, 0xa5, 0x66, 0xaf, 0x0f, 0x94, 0xc3, 0xeb, 0,
        ],
    )
    .unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/batches/strings/compare/fixtures/p2-cpu-parallel-word-move-compare/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "mixed word move/compare integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
