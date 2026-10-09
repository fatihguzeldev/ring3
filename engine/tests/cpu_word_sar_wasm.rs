use ring3_engine::process::EngineInstance;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_sar_uses_current_low_word_and_masked_counts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(engine.is_file());
    let parent = root.join("target/p2-word-sar-fixtures");
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
    let mut bank = vec![0xcc; 1024];
    let mut at = 0;
    for opcode in [0xd1, 0xc1] {
        for alias in 0..8 {
            bank[at..at + 3].copy_from_slice(&[0x66, opcode, 0xf8 | alias]);
            at += 3;
            if opcode == 0xc1 {
                bank[at] = 0xff;
                at += 1;
            }
        }
    }
    for raw in [0, 1, 2, 15, 16, 17, 31, 32, 33] {
        bank[at..at + 4].copy_from_slice(&[0x66, 0xc1, 0xf8, raw]);
        at += 4;
    }
    for alias in 0..8 {
        bank[at..at + 3].copy_from_slice(&[0x66, 0xd3, 0xf8 | alias]);
        at += 3;
    }
    assert_eq!(at, 116);
    bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    let chain = [
        0x66, 0xb9, 0x21, 0, 0x66, 0xd3, 0xf9, 0x66, 0xd3, 0xf9, 0x66, 0x83, 0xf8, 1, 0xf9, 0x66,
        0xb9, 0x20, 0, 0x66, 0xd3, 0xfc, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x66, 0xc1, 0xff,
        0x10, 0x66, 0x83, 0xd5, 0, 0x66, 0xb9, 0, 0, 0x66, 0xd3, 0xf8, 0x0f, 0x92, 0xc1, 0x66,
        0xd3, 0xf8, 0x0f, 0x90, 0xc3, 0x75, 2, 0x0f, 0x0b, 0xeb, 0,
    ];
    assert_eq!(chain.len(), 58);
    bank[512..570].copy_from_slice(&chain);
    for (name, bytes) in [
        ("word-sar.x86", bank),
        (
            "initial-arena.bin",
            EngineInstance::new(1, 1).unwrap().arena().to_vec(),
        ),
    ] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))
            .unwrap()
            .write_all(&bytes)
            .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-sar/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual SAR integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
