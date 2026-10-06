use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::CompileLimits;

#[test]
fn actual_wasm_memory_cmpxchg_writes_both_paths_and_resumes_current_tails() {
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
        .join("target/p2-memory-cmpxchg32-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    let normal: Vec<Vec<u8>> = (0u8..8)
        .map(|source| vec![0x0f, 0xb1, 0x05 | source << 3, 0x10, 0x40, 0, 0])
        .collect();
    let special = vec![
        vec![0x0f, 0xb1, 0x08],
        vec![0x0f, 0xb1, 0x44, 0x87, 0xf0],
        vec![0x0f, 0xb1, 0x04, 0x80],
        vec![0x0f, 0xb1, 0x24, 0x24],
        vec![0x0f, 0xb1, 0x6d, 0],
        vec![0x83, 0xc3, 1, 0x0f, 0xb1, 0x0f],
        vec![0xbe, 0xff, 0xff, 0xff, 0x7f, 0x0f, 0xb1, 0x0f],
    ];
    for (name, forms, packed, entries) in [
        ("normal", normal, 120, vec![0, 15, 30, 45, 60, 75, 90, 105]),
        ("special", special, 95, vec![0, 11, 24, 36, 48, 60, 74]),
    ] {
        let mut bytes = [0xcc; 130];
        let mut offset = 0;
        let mut actual_entries = Vec::new();
        for (index, form) in forms.iter().enumerate() {
            actual_entries.push(offset);
            let mut tail = vec![0x0f, 0x94, 0xc0, 0x0f, 0x92, 0xc2];
            if name == "special" && index == 6 {
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
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-cmpxchg32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory CMPXCHG32 integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
