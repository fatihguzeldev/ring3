use ring3_engine::process::EngineInstance;
use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_word_register_transfers_preserve_current_parent_flags_and_fp() {
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
    let parent = root.join("target/p2-word-register-move-fixtures");
    fs::create_dir_all(&parent).unwrap();
    let output = parent.join(unique);
    fs::create_dir(&output).unwrap();
    let mut bank = vec![0xcc; 512];
    for group in 0..4_usize {
        let opcode = if group < 2 { 0x89 } else { 0x8b };
        for slot in 0..4_usize {
            for source in 0..8_u8 {
                let destination = (group % 2 * 4 + slot) as u8;
                let modrm = if opcode == 0x89 {
                    (source << 3) | destination
                } else {
                    (destination << 3) | source
                };
                let at = group * 128 + slot * 32 + usize::from(source) * 3;
                bank[at..at + 3].copy_from_slice(&[0x66, opcode, 0xc0 | modrm]);
            }
            bank[group * 128 + slot * 32 + 24..group * 128 + slot * 32 + 26]
                .copy_from_slice(&[0xeb, 0]);
        }
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("word-register-move.x86"))
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
        .arg(root.join("engine/tests/fixtures/p2-word-register-move/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual word register move integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
