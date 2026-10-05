use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_wasm_accepts_only_masked_one_memory_byte_shift_immediates() {
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
        .join("target/p2-memory-byte-shift-immediate-one-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let counts = [1, 33, 65, 97, 129, 161, 193, 225];
    for (kind, extension) in [("shl", 4 << 3), ("shr", 5 << 3), ("sar", 7 << 3)] {
        let mut bytes = vec![0xcc; 144];
        let forms = [
            vec![0x83, 0xc3, 1, 0xc0, extension, counts[0]],
            vec![0xc0, 0x41 | extension, 0xf0, counts[1]],
            vec![0xc0, 0x44 | extension, 0x4a, 0x11, counts[2]],
            vec![
                0xc0,
                0x84 | extension,
                0xf3,
                0xe0,
                0xff,
                0xff,
                0xff,
                counts[3],
            ],
            vec![0xc0, 0x04 | extension, 0x24, counts[4]],
            vec![0xc0, 0x45 | extension, 0, counts[5]],
            vec![0x8d, 0x5b, 1, 0xc0, 0x06 | extension, counts[6]],
            vec![0xc0, 0x87 | extension, 0, 1, 0, 0, counts[7]],
        ];
        let mut offset = 0;
        for (index, form) in forms.iter().enumerate() {
            let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
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
        assert_eq!(offset, 113);
        bytes[128..130].copy_from_slice(&[0x0f, 0x0b]);
        bytes[130..].copy_from_slice(&[
            0xd0,
            5 | extension,
            0x10,
            0x40,
            0,
            0,
            0x0f,
            0x92,
            0xc0,
            0x0f,
            0x90,
            0xc2,
            0xeb,
            0xf0,
        ]);
        fs::write(output.join(format!("memory-{kind}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-shift-immediate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual masked-one memory byte shift integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
