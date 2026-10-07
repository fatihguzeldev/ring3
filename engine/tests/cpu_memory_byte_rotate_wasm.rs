use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::CompileLimits;

#[test]
fn actual_wasm_memory_byte_rotates_separate_count_policy_from_distance() {
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
        .join("target/p2-memory-byte-rotate-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for (kind, extension) in [("rol", 0u8), ("ror", 8u8)] {
        let mut normal_a: Vec<Vec<u8>> = [0, 1, 2, 7, 8, 9, 16]
            .into_iter()
            .map(|raw| vec![0xc0, 5 | extension, 0x10, 0x40, 0, 0, raw])
            .collect();
        normal_a.push(vec![0xd2, 5 | extension, 0x10, 0x40, 0, 0]);
        save_bank(
            &output,
            &format!("normal-a-{kind}"),
            normal_a,
            None,
            119,
            vec![0, 15, 30, 45, 60, 75, 90, 105],
        );
        let normal_b = [17, 24, 31, 32, 33, 255]
            .into_iter()
            .map(|raw| vec![0xc0, 5 | extension, 0x10, 0x40, 0, 0, raw])
            .collect();
        save_bank(
            &output,
            &format!("normal-b-{kind}"),
            normal_b,
            None,
            90,
            vec![0, 15, 30, 45, 60, 75],
        );
        let fault_target = if kind == "rol" {
            vec![0xc0, 7, 32]
        } else {
            vec![0xd2, 0x0f]
        };
        let mut fault = vec![0x83, 0xc3, 1];
        fault.extend_from_slice(&fault_target);
        let special = vec![
            vec![0xd2, 1 | extension],
            vec![0xd2, 0x44 | extension, 0x8f, 0xf0],
            vec![0xd2, 4 | extension, 0x89],
            vec![0xd2, 4 | extension, 0x24],
            vec![0xd2, 0x45 | extension, 0],
            fault,
        ];
        save_bank(
            &output,
            &format!("special-{kind}"),
            special,
            None,
            if kind == "rol" { 69 } else { 68 },
            vec![0, 10, 22, 33, 44, 55],
        );
        let mut zero = vec![0xbe, 0, 0x56, 0x34, 0x12];
        zero.extend_from_slice(&[0xc0, 7 | extension, 32]);
        let mut equal = vec![0xbe, 0x80, 0x56, 0x34, 0x12];
        equal.extend_from_slice(&[0xc0, 7 | extension, 8]);
        let mut changing = vec![
            0xbe,
            if kind == "rol" { 0x81 } else { 0x80 },
            0x56,
            0x34,
            0x12,
        ];
        changing.extend_from_slice(&[0xd2, 7 | extension]);
        save_bank(
            &output,
            &format!("smc-{kind}"),
            vec![zero, equal, changing],
            Some(0),
            62,
            vec![0, 21, 42],
        );
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-rotate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory byte rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}

fn save_bank(
    output: &Path,
    name: &str,
    forms: Vec<Vec<u8>>,
    all_canaries: Option<usize>,
    packed: usize,
    entries: Vec<usize>,
) {
    let mut bytes = [0xcc; 130];
    let mut offset = 0;
    let mut actual_entries = Vec::new();
    for form in &forms {
        actual_entries.push(offset);
        let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
        if all_canaries.is_some() {
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
