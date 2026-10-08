use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const PC: u32 = 0x1000;
const COLD: u32 = 0x1f00;

fn bank(extension: u8) -> Vec<u8> {
    let mut bytes = vec![0xcc; 4096];
    let mut at = 0usize;
    let mut append = |form: &[u8]| {
        let mut code = form.to_vec();
        code.extend_from_slice(&[0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xe9]);
        let next = PC + u32::try_from(at + code.len() + 4).unwrap();
        code.extend_from_slice(
            &i32::try_from(i64::from(COLD) - i64::from(next))
                .unwrap()
                .to_le_bytes(),
        );
        bytes[at..at + code.len()].copy_from_slice(&code);
        at += code.len();
    };
    for form in [
        vec![0xd2, 0x03 | extension],
        vec![0xd2, 0x01 | extension],
        vec![0xd2, 0x04 | extension, 0x89],
        vec![0xd2, 0x44 | extension, 0x8f, 0xf0],
        vec![0xd2, 0x04 | extension, 0x24],
        vec![0xd2, 0x07 | extension],
    ] {
        append(&form);
    }
    for raw in [1, 32, 2] {
        append(&[0xb1, raw, 0x8d, 0x5b, 1, 0xd2, 0x03 | extension]);
    }
    append(&[
        0xb1,
        1,
        0xd2,
        0x03 | extension,
        0xb1,
        7,
        0xd2,
        0x03 | extension,
    ]);
    assert_eq!(at, 155);
    for (slot, value) in [0x81, 0, 0x81].into_iter().enumerate() {
        let start = 0xe00 + slot * 32;
        let mut code = vec![
            0xb0,
            value,
            0xd2,
            0x07 | extension,
            0x0f,
            0x92,
            0xc0,
            0x0f,
            0x90,
            0xc2,
            0xbb,
            0xef,
            0xbe,
            0xad,
            0xde,
            0xe9,
        ];
        let next = PC + u32::try_from(start + code.len() + 4).unwrap();
        code.extend_from_slice(
            &i32::try_from(i64::from(COLD) - i64::from(next))
                .unwrap()
                .to_le_bytes(),
        );
        assert_eq!(code.len(), 20);
        bytes[start..start + code.len()].copy_from_slice(&code);
    }
    bytes[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    bytes
}

#[test]
fn actual_engine_memory_byte_shift_cl_counts_flags_faults_and_currency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(
        engine.is_file(),
        "build the current engine wasm32 cdylib first"
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
        .join("target/p2-memory-byte-shift-cl-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    for (kind, extension) in [("shl", 0x20), ("shr", 0x28), ("sar", 0x38)] {
        fs::write(output.join(format!("memory-{kind}.x86")), bank(extension)).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-shift-cl/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory byte shift CL integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
