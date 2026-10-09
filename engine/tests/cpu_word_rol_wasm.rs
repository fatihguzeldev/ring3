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
fn actual_word_rol_preserves_unaffected_flags_and_uses_current_low16_cl() {
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
    let parent = root.join("target/p2-word-rol-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(unique);
    fs::create_dir(&output).unwrap();
    let mut bank = vec![0xcc; 512];
    let mut at = 0;
    for opcode in [0xd1, 0xc1, 0xd3] {
        for destination in 0..8_u8 {
            bank[at..at + 3].copy_from_slice(&[0x66, opcode, 0xc0 | destination]);
            at += 3;
            if opcode == 0xc1 {
                bank[at] = 255;
                at += 1;
            }
        }
    }
    for raw in [0, 1, 15, 16, 17, 31, 32, 33] {
        bank[at..at + 4].copy_from_slice(&[0x66, 0xc1, 0xc0, raw]);
        at += 4;
    }
    assert_eq!(at, 112);
    bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    let chain = [
        0x66, 0xb8, 0, 0x80, 0x66, 0xc1, 0xc0, 0, 0x0f, 0x90, 0xc3, 0x66, 0xc1, 0xc0, 16, 0x0f,
        0x92, 0xc7, 0x66, 0xc1, 0xc0, 17, 0x66, 0x83, 0xd0, 0, 0x66, 0xb9, 0x21, 0, 0x66, 0xd3,
        0xc1, 0x66, 0xd3, 0xc1, 0x74, 2,
    ];
    assert_eq!(chain.len(), 38);
    bank[256..256 + chain.len()].copy_from_slice(&chain);
    write_new(&output, "word-rol.x86", &bank);
    write_new(
        &output,
        "initial-arena.bin",
        EngineInstance::new(2, 1).unwrap().arena(),
    );
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-rol/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word rol failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
