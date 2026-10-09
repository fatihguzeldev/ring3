use ring3_engine::process::EngineInstance;
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn write_new(directory: &Path, name: &str, bytes: &[u8]) {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn actual_word_logical_shifts_use_current_low16_cl_and_count_flags() {
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
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let parent = root.join("target/p2-word-logical-shift-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(unique);
    fs::create_dir(&output).unwrap();
    let counts = [0, 1, 2, 15, 16, 17, 31, 32, 33, 255];
    let mut bank = vec![0xcc; 1024];
    for (opcode, offset) in [(0xd1, 0), (0xc1, 256), (0xd3, 512)] {
        let mut at = offset;
        for extension in [4, 5] {
            for destination in 0..8_u8 {
                for pass in 0..if opcode == 0xc1 { 2 } else { 1 } {
                    let instruction = [0x66, opcode, 0xc0 | extension << 3 | destination];
                    bank[at..at + 3].copy_from_slice(&instruction);
                    at += 3;
                    if opcode == 0xc1 {
                        bank[at] = counts[(destination as usize + pass * 8) % counts.len()];
                        at += 1;
                    }
                }
            }
        }
        assert_eq!(at - offset, if opcode == 0xc1 { 128 } else { 48 });
        bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    }
    let chain = [
        0x66, 0xb9, 0x21, 0, 0x66, 0xd3, 0xe1, 0x66, 0xd3, 0xe9, 0x66, 0xc1, 0xe1, 0, 0x0f, 0x92,
        0xc0, 0x66, 0x83, 0xd0, 0, 0xeb, 0,
    ];
    bank[768..768 + chain.len()].copy_from_slice(&chain);
    write_new(&output, "word-logical-shift.x86", &bank);
    write_new(
        &output,
        "initial-arena.bin",
        EngineInstance::new(2, 1).unwrap().arena(),
    );
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-logical-shift/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word logical shifts failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
