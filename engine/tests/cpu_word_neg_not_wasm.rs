use ring3_engine::process::EngineInstance;
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_neg_not_preserves_parents_and_consumes_current_flags() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pinned engine sha256 is required");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(engine.is_file());
    let parent = root.join("target/p2-word-neg-not-fixtures");
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
    for extension in [3_u8, 2] {
        for destination in 0..8_u8 {
            bank[at..at + 3].copy_from_slice(&[0x66, 0xf7, 0xc0 | extension << 3 | destination]);
            at += 3;
        }
    }
    assert_eq!(at, 48);
    bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    let chain = [
        0xf9, 0x66, 0xb8, 0, 0x80, 0x66, 0xf7, 0xd8, 0x0f, 0x90, 0xc1, 0x0f, 0x92, 0xc2, 0x66,
        0xf7, 0xd0, 0x0f, 0x90, 0xc5, 0x0f, 0x92, 0xc6, 0x66, 0xbd, 0, 0, 0x66, 0xf7, 0xdd, 0x0f,
        0x94, 0xc3, 0x0f, 0x92, 0xc4, 0x74, 2, 0x0f, 0x0b, 0xeb, 0,
    ];
    assert_eq!(chain.len(), 42);
    bank[768..768 + chain.len()].copy_from_slice(&chain);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("word-neg-not.x86"))
        .unwrap()
        .write_all(&bank)
        .unwrap();
    let initial = EngineInstance::new(2, 1).unwrap().arena().to_vec();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("initial-arena.bin"))
        .unwrap()
        .write_all(&initial)
        .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-neg-not/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word neg/not integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
