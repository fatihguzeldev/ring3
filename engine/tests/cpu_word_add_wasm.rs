use ring3_engine::process::EngineInstance;
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_add_wraps_low16_and_preserves_parent_high16_and_fp() {
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
    let parent = root.join("target/p2-word-add-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(unique);
    fs::create_dir(&output).unwrap();
    let mut forms = Vec::new();
    for opcode in [0x01, 0x03] {
        for left in 0..8_u8 {
            for right in [left, (left + 1) & 7] {
                let modrm = if opcode == 0x01 {
                    (right << 3) | left
                } else {
                    (left << 3) | right
                };
                forms.push(vec![0x66, opcode, 0xc0 | modrm]);
            }
        }
    }
    for left in 0..8_u8 {
        for immediate in [0_u16, 0x7fff, 0x8000, 0xffff] {
            let mut bytes = vec![0x66, 0x81, 0xc0 | left];
            bytes.extend(immediate.to_le_bytes());
            forms.push(bytes);
        }
    }
    for left in 0..8_u8 {
        for immediate in [0_u8, 0x7f, 0x80, 0xff] {
            forms.push(vec![0x66, 0x83, 0xc0 | left, immediate]);
        }
    }
    for immediate in [
        0_u16, 0x7fff, 0x8000, 0xffff, 0x6667, 0x6766, 0xf0f3, 0xf3f2,
    ] {
        let mut bytes = vec![0x66, 0x05];
        bytes.extend(immediate.to_le_bytes());
        forms.push(bytes);
    }
    assert_eq!(forms.len(), 104);
    let mut bank = vec![0xcc; 2048];
    for (group, chunk) in forms.chunks(32).enumerate() {
        let mut at = group * 256;
        for instruction in chunk {
            bank[at..at + instruction.len()].copy_from_slice(instruction);
            at += instruction.len();
        }
        bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    }
    let chain = [
        0x66, 0xb8, 0xff, 0x7f, 0x66, 0x83, 0xc0, 1, 0x0f, 0x90, 0xc2, 0x66, 0xb9, 0xff, 0xff,
        0x66, 0x01, 0xc8, 0x0f, 0x92, 0xc3, 0xeb, 0,
    ];
    bank[1792..1792 + chain.len()].copy_from_slice(&chain);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("word-add.x86"))
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
        .arg(root.join("engine/tests/fixtures/p2-word-add/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word add integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
