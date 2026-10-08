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
    let mut append = |form: &[u8], canary: bool| {
        let mut code = form.to_vec();
        code.extend_from_slice(&[0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2]);
        if canary {
            code.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
        }
        code.push(0xe9);
        let next = PC + u32::try_from(at + code.len() + 4).unwrap();
        let relative = i64::from(COLD) - i64::from(next);
        code.extend_from_slice(&i32::try_from(relative).unwrap().to_le_bytes());
        bytes[at..at + code.len()].copy_from_slice(&code);
        at += code.len();
    };
    for raw in [0, 1, 2, 7, 8, 9, 31, 32, 33, 255] {
        append(&[0xc0, 5 | extension, 0x10, 0x40, 0, 0, raw], false);
    }
    for form in [
        vec![0xc0, extension, 0],
        vec![0xc0, 0x41 | extension, 0xf0, 1],
        vec![0xc0, 0x44 | extension, 0x4a, 0x11, 7],
        vec![0xc0, 0x84 | extension, 0xf3, 0xe0, 0xff, 0xff, 0xff, 8],
        vec![0xc0, 0x04 | extension, 0x24, 9],
        vec![0xc0, 0x45 | extension, 0, 31],
        vec![0xc0, 0x06 | extension, 32],
        vec![0xc0, 0x87 | extension, 0, 1, 0, 0, 255],
    ] {
        append(&form, false);
    }
    for raw in [0, 1, 8] {
        append(&[0x8d, 0x5b, 1, 0xc0, 0x07 | extension, raw], false);
    }
    append(
        &[
            0x83,
            0xc3,
            1,
            0xc0,
            0x06 | extension,
            1,
            0xc0,
            0x06 | extension,
            7,
        ],
        false,
    );
    assert_eq!(at, 377);
    for (slot, (raw, value)) in [0, 1, 8]
        .into_iter()
        .flat_map(|raw| [0, 0x81].into_iter().map(move |value| (raw, value)))
        .enumerate()
    {
        let start = 0xe00 + slot * 32;
        let mut code = vec![
            0xb0,
            value,
            0xc0,
            0x07 | extension,
            raw,
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
        assert_eq!(code.len(), 21);
        bytes[start..start + code.len()].copy_from_slice(&code);
    }
    bytes[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    bytes
}

#[test]
fn actual_engine_memory_byte_shift_immediate_counts_flags_faults_and_currency() {
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
        .join("target/p2-memory-byte-shift-immediate-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    for (kind, extension) in [("shl", 0x20), ("shr", 0x28), ("sar", 0x38)] {
        fs::write(output.join(format!("memory-{kind}.x86")), bank(extension)).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-shift-immediate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "memory byte shift immediate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
