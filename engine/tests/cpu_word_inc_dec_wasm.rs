use ring3_engine::process::EngineInstance;
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_inc_dec_preserves_cf_and_parent_high16_with_current_operands() {
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
    let parent = root.join("target/p2-word-inc-dec-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(unique);
    fs::create_dir(&output).unwrap();
    let mut forms = Vec::new();
    for (short, extension) in [(0x40, 0), (0x48, 1)] {
        for destination in 0..8_u8 {
            forms.push(vec![0x66, short + destination]);
            forms.push(vec![0x66, 0xff, 0xc0 | extension << 3 | destination]);
        }
    }
    assert_eq!(forms.len(), 32);
    let mut bank = vec![0xcc; 1024];
    let mut at = 0;
    for instruction in forms {
        bank[at..at + instruction.len()].copy_from_slice(&instruction);
        at += instruction.len();
    }
    assert_eq!(at, 80);
    bank[at..at + 2].copy_from_slice(&[0xeb, 0]);
    let chain = [
        0xf9, 0x66, 0xb8, 0xff, 0xff, 0x66, 0x40, 0x0f, 0x92, 0xc2, 0x66, 0x15, 0, 0, 0xf8, 0x66,
        0xb9, 0, 0, 0x66, 0xff, 0xc9, 0x0f, 0x92, 0xc6, 0x66, 0x83, 0xd9, 0xff, 0x66, 0xb8, 0xff,
        0x7f, 0x66, 0xff, 0xc0, 0x0f, 0x90, 0xc3, 0x66, 0xb9, 0, 0x80, 0x66, 0x49, 0x0f, 0x90,
        0xc7, 0xeb, 0,
    ];
    bank[768..768 + chain.len()].copy_from_slice(&chain);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("word-inc-dec.x86"))
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
        .arg(root.join("engine/tests/fixtures/p2-word-inc-dec/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word inc/dec integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
