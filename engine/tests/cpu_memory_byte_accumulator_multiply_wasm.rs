use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_memory_byte_products_read_old_ea_and_repaired_current_al() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin the actual engine SHA-256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let output = root
        .join("target/p2-memory-byte-accumulator-multiply-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 1).unwrap().arena(),
    )
    .unwrap();
    let tails: [&[u8]; 8] = [
        &[0x03],
        &[0x00],
        &[0x02],
        &[0x04, 0x24],
        &[0x44, 0x8f, 0x80],
        &[0x45, 0x80],
        &[0x83, 0x10, 0, 0, 0],
        &[0x05, 0x10, 0x50, 0, 0],
    ];
    for (kind, extension) in [("unsigned", 0x20), ("signed", 0x28)] {
        let mut bytes = vec![0xcc; 128];
        for (index, tail) in tails.iter().enumerate() {
            let at = index * 16;
            bytes[at] = 0xf6;
            bytes[at + 1..at + 1 + tail.len()].copy_from_slice(tail);
            bytes[at + 1] |= extension;
            let end = at + 1 + tail.len();
            bytes[end] = 0xe9;
            bytes[end + 1..end + 5]
                .copy_from_slice(&(0x1800_i32 - (0x1000 + end as i32 + 5)).to_le_bytes());
        }
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
    }
    fs::write(
        output.join("chain.x86"),
        [
            0xb8, 0x11, 0, 0, 0x50, 0xf6, 0x20, 0xf6, 0x23, 0xf6, 0x23, 0xeb, 0,
        ],
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-accumulator-multiply/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .arg(ARENA_SIZE.to_string())
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory BYTE multiply integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
