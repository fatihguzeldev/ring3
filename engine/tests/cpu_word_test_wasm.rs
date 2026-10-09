use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn banks() -> [Vec<u8>; 4] {
    let mut registers = Vec::new();
    let mut modrm = Vec::new();
    let mut accumulator = Vec::new();
    for alias in 0..8 {
        for right in [alias, (alias + 1) & 7] {
            registers.extend([0x66, 0x85, 0xc0 | right << 3 | alias]);
        }
        for value in [0_u16, 0x7fff, 0x8000, 0xffff] {
            let [lo, hi] = value.to_le_bytes();
            modrm.extend([0x66, 0xf7, 0xc0 | alias, lo, hi]);
        }
    }
    for value in [
        0_u16, 0x7fff, 0x8000, 0xffff, 0x6667, 0x6766, 0xf0f3, 0xf3f2,
    ] {
        let [lo, hi] = value.to_le_bytes();
        accumulator.extend([0x66, 0xa9, lo, hi]);
    }
    for bank in [&mut registers, &mut modrm, &mut accumulator] {
        bank.extend([0xeb, 0, 0x0f, 0x0b]);
    }
    let chain = vec![
        0x66, 0xb8, 0, 0x80, 0x66, 0xa9, 0xff, 0xff, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x0f,
        0x94, 0xc3, 0x75, 2, 0x0f, 0x0b, 0x0f, 0x0b,
    ];
    assert_eq!(
        [registers.len(), modrm.len(), accumulator.len(), chain.len()],
        [52, 164, 36, 23]
    );
    [registers, modrm, accumulator, chain]
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
fn actual_word_test_preserves_parents_and_publishes_width16_flags() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine first");
    let expected = std::env::var("RING3_ENGINE_SHA256").expect("pin integrated engine SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let output = root.join("target/p2-word-test-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::create_dir(&output).unwrap();
    for (name, bytes) in ["registers", "modrm", "accumulator", "chain"]
        .into_iter()
        .zip(banks())
    {
        save(&output.join(format!("{name}.x86")), &bytes);
    }
    let initial = EngineInstance::new(4, 0x574f_5244_5445_5354).unwrap();
    assert_eq!(ARENA_SIZE, 4364);
    save(&output.join("initial-arena.bin"), initial.arena());
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-test/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "WORD TEST fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
