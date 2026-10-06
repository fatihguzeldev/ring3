use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::CompileLimits;

#[test]
fn actual_wasm_memory_double_shifts_preserve_checked_rmw_and_current_tail_continuity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root
        .join("target/p2-memory-double-shift32-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for (kind, immediate, cl) in [("shld", 0xa4, 0xa5), ("shrd", 0xac, 0xad)] {
        for mode in ["imm", "cl"] {
            let opcode = if mode == "imm" { immediate } else { cl };
            let normal: Vec<Vec<u8>> = (0u8..8)
                .map(|source| {
                    let mut bytes = vec![0x0f, opcode, 0x05 + source * 8, 0x10, 0x40, 0, 0];
                    if mode == "imm" {
                        bytes.push(1);
                    }
                    bytes
                })
                .collect();
            let edge: Vec<Vec<u8>> = [0, 32, 64, 128, 2, 31, 33, 255]
                .into_iter()
                .map(|raw| {
                    if mode == "imm" {
                        vec![0x0f, immediate, 0x37, raw]
                    } else {
                        vec![0x0f, cl, 0x0f]
                    }
                })
                .collect();
            for (group, forms, packed, stride) in [
                (
                    "normal",
                    normal,
                    if mode == "imm" { 128 } else { 120 },
                    if mode == "imm" { 16 } else { 15 },
                ),
                (
                    "edge",
                    edge,
                    if mode == "imm" { 96 } else { 88 },
                    if mode == "imm" { 12 } else { 11 },
                ),
            ] {
                save_bank(
                    &output,
                    &format!("{group}-{kind}-{mode}"),
                    forms,
                    None,
                    packed,
                    (0..8).map(|index| index * stride).collect(),
                );
            }
        }
        let fault = if kind == "shld" {
            vec![0x83, 0xc3, 1, 0x0f, immediate, 0x0f, 32]
        } else {
            vec![0x83, 0xc3, 1, 0x0f, cl, 0x0f]
        };
        let smc = if kind == "shld" {
            vec![0xbe, 0x78, 0x56, 0x34, 0x12, 0x0f, immediate, 0x0f, 32]
        } else {
            vec![0xbe, 1, 0, 0, 0, 0x0f, immediate, 0x0f, 1]
        };
        let special = vec![
            vec![0x0f, cl, 0],
            vec![0x0f, immediate, 0x24, 0x24, 1],
            vec![0x0f, immediate, 0x6d, 0, 1],
            vec![0x0f, cl, 0x4c, 0x8f, 0xf0],
            vec![0x0f, immediate, 0x1c, 0x9b, 1],
            fault,
            smc,
        ];
        save_bank(
            &output,
            &format!("special-{kind}"),
            special,
            Some(6),
            if kind == "shld" { 100 } else { 99 },
            vec![0, 11, 24, 37, 50, 63, if kind == "shld" { 78 } else { 77 }],
        );
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-double-shift32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory double shift integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

fn save_bank(
    output: &Path,
    name: &str,
    forms: Vec<Vec<u8>>,
    canary: Option<usize>,
    packed: usize,
    entries: Vec<usize>,
) {
    let mut bytes = [0xcc; 130];
    let mut offset = 0;
    let mut actual_entries = Vec::new();
    for (index, form) in forms.iter().enumerate() {
        actual_entries.push(offset);
        let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
        if canary == Some(index) {
            tail.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
        }
        let length = form.len() + tail.len() + 2;
        bytes[offset..offset + form.len()].copy_from_slice(form);
        bytes[offset + form.len()..offset + length - 2].copy_from_slice(&tail);
        bytes[offset + length - 2..offset + length]
            .copy_from_slice(&[0xeb, u8::try_from(128 - offset - length).unwrap()]);
        offset += length;
    }
    assert_eq!(offset, packed);
    assert_eq!(actual_entries, entries);
    assert!(forms.len() <= 8);
    bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
    fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
}
