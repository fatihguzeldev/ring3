use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_wasm_memory_rotate_commits_after_checked_read_and_store() {
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
        .join("target/p2-memory-rotate-one-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    for (kind, extension) in [("left", 0), ("right", 8)] {
        let forms = [
            vec![0x83, 0xc3, 1, 0xd1, 0x05 | extension, 0x10, 0x40, 0, 0],
            vec![0xd1, 0x40 | extension, 0x10],
            vec![0xd1, 0x42 | extension, 0xfe],
            vec![0xd1, 0x44 | extension, 0x24, 8],
            vec![0xd1, 0x44 | extension, 0x87, 0x10],
            vec![0xd1, 0x44 | extension, 0x57, 0xfe],
            vec![0x8d, 0x5b, 1, 0xd1, 0x07 | extension],
            vec![0xd1, 0x07 | extension],
        ];
        let mut bytes = [0xcc; 130];
        let mut offset = 0;
        for (index, form) in forms.iter().enumerate() {
            let tail: &[u8] = if index == 7 {
                &[0xbb, 0xef, 0xbe, 0xad, 0xde]
            } else {
                &[0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2]
            };
            let length = form.len() + tail.len() + 2;
            bytes[offset..offset + form.len()].copy_from_slice(form);
            bytes[offset + form.len()..offset + length - 2].copy_from_slice(tail);
            bytes[offset + length - 2..offset + length]
                .copy_from_slice(&[0xeb, u8::try_from(128 - offset - length).unwrap()]);
            offset += length;
        }
        assert_eq!(offset, 97);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-rotate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual count-one memory rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
