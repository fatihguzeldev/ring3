use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_engine_retains_word_rotates_and_flag_consumers() {
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
    let parent = root.join("target/p2-cpu-parallel-word-rotate-fixtures");
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
        .open(output.join("word-rotate.x86"))
        .unwrap()
        .write_all(&[
            0x66, 0xb9, 0x21, 0x00, 0x66, 0xd3, 0xc1, 0x66, 0xd3, 0xc9, 0x0f, 0x92, 0xc2, 0x0f,
            0x94, 0xc7, 0x66, 0x83, 0xf8, 0x01, 0xf9, 0x66, 0xb9, 0x20, 0x00, 0x66, 0xd3, 0xc4,
            0x0f, 0x92, 0xc6, 0x0f, 0x90, 0xc3, 0x66, 0xc1, 0xc6, 0x10, 0x0f, 0x90, 0xc7, 0x66,
            0x83, 0xd2, 0x00, 0x66, 0xc1, 0xcf, 0x10, 0x66, 0x83, 0xdd, 0x00, 0x66, 0xb8, 0x00,
            0x80, 0x66, 0xd1, 0xc8, 0x0f, 0x90, 0xc5, 0x66, 0xb8, 0x00, 0x80, 0x66, 0xc1, 0xc8,
            0x11, 0x0f, 0x90, 0xc7, 0x66, 0xbb, 0x00, 0x40, 0x66, 0xd1, 0xc3, 0x0f, 0x90, 0xc1,
            0x66, 0xbb, 0x00, 0x40, 0x66, 0xc1, 0xc3, 0x11, 0x71, 0x02, 0x0f, 0x0b, 0xeb, 0x00,
        ])
        .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-cpu-parallel-word-rotate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "mixed word rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
