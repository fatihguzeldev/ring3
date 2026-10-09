use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_retains_word_arithmetic_and_flag_consumers() {
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
    let parent = root.join("target/p2-cpu-parallel-word-arithmetic-fixtures");
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
        .open(output.join("word-arithmetic.x86"))
        .unwrap()
        .write_all(&[
            0x66, 0x01, 0xc8, 0x0f, 0x92, 0xc2, 0x66, 0x29, 0xc8, 0x66, 0x83, 0xf8, 0xff, 0x74,
            0x02, 0x0f, 0x0b, 0x66, 0x83, 0xc6, 0x01, 0x0f, 0x90, 0xc3, 0x66, 0x83, 0xec, 0x01,
            0x66, 0x83, 0xe9, 0xff, 0x66, 0x03, 0xc9, 0x66, 0x83, 0xf9, 0x04, 0xeb, 0x00,
        ])
        .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-cpu-parallel-word-arithmetic/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "mixed word arithmetic integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
