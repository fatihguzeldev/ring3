use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn banks() -> [Vec<u8>; 3] {
    let mut opcode = Vec::new();
    let mut modrm = Vec::new();
    for alias in 0..8 {
        for value in [0_u16, 0x7fff, 0x8000, 0xffff] {
            let [low, high] = value.to_le_bytes();
            opcode.extend([0x66, 0xb8 | alias, low, high]);
            modrm.extend([0x66, 0xc7, 0xc0 | alias, low, high]);
        }
    }
    let mut payload = vec![
        0x66, 0xb8, 0x67, 0x66, 0x66, 0xc7, 0xc0, 0xf3, 0xf0, 0x66, 0xbc, 0xf3, 0xf0, 0x66, 0xc7,
        0xc4, 0x67, 0x66,
    ];
    for bytes in [&mut opcode, &mut modrm, &mut payload] {
        bytes.extend([0xeb, 0, 0x0f, 0x0b]);
    }
    assert_eq!([opcode.len(), modrm.len(), payload.len()], [132, 164, 22]);
    [opcode, modrm, payload]
}

fn save(path: &Path, bytes: &[u8]) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

#[test]
fn actual_word_move_immediate_preserves_current_parent_and_flags() {
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
    let output = root
        .join("target/p2-word-move-immediate-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::create_dir(&output).unwrap();
    for (name, bytes) in ["opcode.x86", "modrm.x86", "payload.x86"]
        .into_iter()
        .zip(banks())
    {
        save(&output.join(name), &bytes);
    }
    let initial = EngineInstance::new(3, 0x5749_4d4d_4544_0001).unwrap();
    assert_eq!(ARENA_SIZE, 4364);
    save(&output.join("initial-arena.bin"), initial.arena());
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-move-immediate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "WORD immediate fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
