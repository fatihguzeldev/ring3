use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::CompileLimits;

#[test]
fn actual_wasm_carry_rotates_memory_bytes_once_after_checked_read_and_store() {
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
        .join("target/p2-memory-byte-carry-rotate-one-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for (kind, extension) in [("left", 0x10), ("right", 0x18)] {
        let mut bytes = vec![0xcc; 130];
        let forms = [
            vec![0xf8, 0xd0, extension],
            vec![0xf9, 0xd0, 0x41 | extension, 0xf0],
            vec![0xf5, 0xd0, 0x44 | extension, 0x4a, 0x11],
            vec![0xd0, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff],
            vec![0xd0, 0x04 | extension, 0x24],
            vec![0xd0, 0x45 | extension, 0],
            vec![0x8d, 0x5b, 1, 0xd0, 0x06 | extension],
            vec![0xd0, 0x87 | extension, 0, 1, 0, 0],
        ];
        let mut offset = 0;
        let mut entries = Vec::new();
        for (index, form) in forms.iter().enumerate() {
            entries.push(offset);
            let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
            if index == 0 {
                tail.extend_from_slice(&[0xd0, 0x05 | extension, 0x10, 0x40, 0, 0]);
            }
            if index == 7 {
                tail.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
            }
            let length = form.len() + tail.len() + 2;
            bytes[offset..offset + form.len()].copy_from_slice(form);
            bytes[offset + form.len()..offset + length - 2].copy_from_slice(&tail);
            bytes[offset + length - 2..offset + length]
                .copy_from_slice(&[0xeb, u8::try_from(128 - offset - length).unwrap()]);
            offset += length;
        }
        assert_eq!(offset, 111);
        assert_eq!(entries, [0, 17, 29, 42, 57, 68, 79, 92]);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        fs::write(output.join(format!("memory-{kind}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-carry-rotate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual count-one memory byte carry rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
