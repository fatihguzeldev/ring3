use super::support_wasm as wasm_support;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_retains_word_unary_and_flag_consumers() {
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
    let parent = root.join("target/p2-cpu-parallel-word-unary-fixtures");
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
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("word-unary.x86"))
        .unwrap()
        .write_all(&[
            0xf9, 0x66, 0x40, 0x0f, 0x90, 0xc1, 0x0f, 0x92, 0xc2, 0x66, 0xf7, 0xd4, 0x0f, 0x94,
            0xc3, 0x66, 0x83, 0xd5, 0x00, 0x66, 0x4e, 0x0f, 0x92, 0xc6, 0x66, 0xf7, 0xdf, 0x0f,
            0x90, 0xc5, 0x66, 0xf7, 0xdd, 0x0f, 0x92, 0xc4, 0x0f, 0x94, 0xc7, 0x74, 0x02, 0x0f,
            0x0b, 0xeb, 0x00,
        ])
        .unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/batches/word/fixtures/p2-cpu-parallel-word-unary/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    assert!(
        result.status.success(),
        "mixed word unary integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
