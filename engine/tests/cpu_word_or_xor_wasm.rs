use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn banks() -> Vec<(&'static str, Vec<u8>)> {
    let mut banks = Vec::new();
    for (names, to_rm, to_reg, accumulator, extension, anchor_value) in [
        (
            [
                "or-registers",
                "or-immediate16",
                "or-immediate8",
                "or-accumulator",
                "or-anchors",
            ],
            0x09,
            0x0b,
            0x0d,
            1,
            0_u16,
        ),
        (
            [
                "xor-registers",
                "xor-immediate16",
                "xor-immediate8",
                "xor-accumulator",
                "xor-anchors",
            ],
            0x31,
            0x33,
            0x35,
            6,
            0xffff_u16,
        ),
    ] {
        let mut registers = Vec::new();
        let mut immediate16 = Vec::new();
        let mut immediate8 = Vec::new();
        let mut immediate = Vec::new();
        for alias in 0..8 {
            for source in [alias, (alias + 1) & 7] {
                registers.extend([0x66, to_rm, 0xc0 | source << 3 | alias]);
                registers.extend([0x66, to_reg, 0xc0 | alias << 3 | source]);
            }
            for value in [0_u16, 0x7fff, 0x8000, 0xffff] {
                let [lo, hi] = value.to_le_bytes();
                immediate16.extend([0x66, 0x81, 0xc0 | extension << 3 | alias, lo, hi]);
            }
            for raw in [0_u8, 0x7f, 0x80, 0xff] {
                immediate8.extend([0x66, 0x83, 0xc0 | extension << 3 | alias, raw]);
            }
        }
        for value in [
            0_u16, 0x7fff, 0x8000, 0xffff, 0x6667, 0x6766, 0xf0f3, 0xf3f2,
        ] {
            let [lo, hi] = value.to_le_bytes();
            immediate.extend([0x66, accumulator, lo, hi]);
        }
        let [lo, hi] = anchor_value.to_le_bytes();
        let mut anchors = vec![0x66, accumulator, lo, hi];
        for bank in [
            &mut registers,
            &mut immediate16,
            &mut immediate8,
            &mut immediate,
            &mut anchors,
        ] {
            bank.extend([0xeb, 0, 0x0f, 0x0b]);
        }
        let values = [registers, immediate16, immediate8, immediate, anchors];
        assert_eq!(
            values.each_ref().map(|bank| bank.len()),
            [100, 164, 132, 36, 8]
        );
        banks.extend(names.into_iter().zip(values));
    }
    banks.push((
        "chain",
        vec![
            0x66, 0xb9, 0x10, 0, 0x66, 0x09, 0xc8, 0x0f, 0x90, 0xc2, 0x66, 0xbb, 0xff, 0xff, 0x66,
            0x31, 0xdb, 0x0f, 0x94, 0xc3, 0x74, 2, 0x0f, 0x0b, 0x0f, 0x0b,
        ],
    ));
    assert_eq!(banks[10].1.len(), 26);
    banks
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
fn actual_word_or_xor_publish_low16_and_clear_logical_flags() {
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
    let output = root.join("target/p2-word-or-xor-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(output.parent().unwrap()).unwrap();
    std::fs::create_dir(&output).unwrap();
    for (name, bytes) in banks() {
        save(&output.join(format!("{name}.x86")), &bytes);
    }
    let initial = EngineInstance::new(12, 0x574f_5244_4f52_584f).unwrap();
    assert_eq!(ARENA_SIZE, 4364);
    save(&output.join("initial-arena.bin"), initial.arena());
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-word-or-xor/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "WORD OR/XOR fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
