use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn banks() -> [Vec<u8>; 6] {
    let mut registers = Vec::new();
    let mut immediate16 = Vec::new();
    let mut immediate8 = Vec::new();
    let mut accumulator = Vec::new();
    for alias in 0..8 {
        for source in [alias, (alias + 1) & 7] {
            registers.extend([0x66, 0x19, 0xc0 | source << 3 | alias]);
            registers.extend([0x66, 0x1b, 0xc0 | alias << 3 | source]);
        }
        for value in [0_u16, 0x7fff, 0x8000, 0xffff] {
            let [lo, hi] = value.to_le_bytes();
            immediate16.extend([0x66, 0x81, 0xd8 | alias, lo, hi]);
        }
        for raw in [0_u8, 0x7f, 0x80, 0xff] {
            immediate8.extend([0x66, 0x83, 0xd8 | alias, raw]);
        }
    }
    for value in [
        0_u16, 0x7fff, 0x8000, 0xffff, 0x6667, 0x6766, 0xf0f3, 0xf3f2,
    ] {
        let [lo, hi] = value.to_le_bytes();
        accumulator.extend([0x66, 0x1d, lo, hi]);
    }
    let mut anchors = vec![0x66, 0x1d, 0xff, 0xff];
    for bank in [
        &mut registers,
        &mut immediate16,
        &mut immediate8,
        &mut accumulator,
        &mut anchors,
    ] {
        bank.extend([0xeb, 0, 0x0f, 0x0b]);
    }
    let chain = vec![
        0x66, 0xb8, 0, 0x80, 0xf9, 0x66, 0x1d, 0, 0, 0x0f, 0x90, 0xc2, 0x66, 0xb9, 0xff, 0xff,
        0xf9, 0x66, 0x19, 0xc8, 0x0f, 0x92, 0xc6, 0x66, 0x19, 0xed, 0x0f, 0x94, 0xc3, 0x75, 2,
        0x0f, 0x0b, 0x0f, 0x0b,
    ];
    assert_eq!(
        [
            registers.len(),
            immediate16.len(),
            immediate8.len(),
            accumulator.len(),
            anchors.len(),
            chain.len()
        ],
        [100, 164, 132, 36, 8, 35]
    );
    [
        registers,
        immediate16,
        immediate8,
        accumulator,
        anchors,
        chain,
    ]
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
fn actual_word_sbb_wraps_low16_and_publishes_six_flags() {
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
    let output = root.join("target/p2-word-sbb-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::create_dir(&output).unwrap();
    for (name, bytes) in [
        "registers",
        "immediate16",
        "immediate8",
        "accumulator",
        "anchors",
        "chain",
    ]
    .into_iter()
    .zip(banks())
    {
        save(&output.join(format!("{name}.x86")), &bytes);
    }
    let initial = EngineInstance::new(6, 0x574f_5244_5342_4221).unwrap();
    assert_eq!(ARENA_SIZE, 4364);
    save(&output.join("initial-arena.bin"), initial.arena());
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-sbb/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "WORD SBB fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
