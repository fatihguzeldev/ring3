use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_retains_word_predicates_and_flag_consumers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let parent = root.join("target/p2-cpu-parallel-word-predicate-fixtures");
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
        .open(output.join("word-predicate.x86"))
        .unwrap()
        .write_all(&[
            0x66, 0xb9, 0x80, 0xff, 0x66, 0x83, 0xf9, 0x80, 0x0f, 0x94, 0xc3, 0x66, 0xb8, 0x00,
            0x80, 0x66, 0x83, 0xf8, 0x01, 0x0f, 0x90, 0xc2, 0xf9, 0x66, 0xa9, 0x00, 0x80, 0x0f,
            0x92, 0xc1, 0x66, 0x85, 0xc9, 0x7a, 0x01, 0x90, 0xeb, 0x00,
        ])
        .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-cpu-parallel-word-predicate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "mixed word predicate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
