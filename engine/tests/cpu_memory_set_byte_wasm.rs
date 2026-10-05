use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn authored_inputs(output: &Path) {
    let shapes: &[&[u8]] = &[
        &[0x00],
        &[0x04, 0x45, 1, 0, 0, 0],
        &[0x41, 0xf0],
        &[0x04, 0x4d, 1, 0, 0, 0],
        &[0x44, 0x54, 0x11],
        &[0x44, 0x95, 0],
        &[0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff],
        &[0x87, 0, 1, 0, 0],
    ];
    let mut canonical = Vec::new();
    let mut ignored = Vec::new();
    let mut addresses = Vec::new();
    for condition in 0_u8..16 {
        canonical.extend_from_slice(&[0x0f, 0x90 + condition, 6]);
        for field in 0_u8..8 {
            ignored.push(vec![0x0f, 0x90 + condition, field << 3 | 6]);
        }
        for (index, tail) in shapes.iter().enumerate() {
            assert_eq!(tail[0] & 0x38, 0);
            let field = (usize::from(condition) + index) % 8;
            let mut bytes = vec![0x0f, 0x90 + condition, tail[0] | (field as u8) << 3];
            bytes.extend_from_slice(&tail[1..]);
            addresses.push(bytes);
        }
    }
    canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    assert_eq!(canonical.len(), 52);
    fs::write(output.join("code-0.x86"), canonical).unwrap();
    for (kind, forms) in [(1, ignored), (4, addresses)] {
        assert_eq!(forms.len(), 128);
        for (index, chunk) in forms.chunks(43).enumerate() {
            let mut bytes: Vec<u8> = chunk.iter().flatten().copied().collect();
            bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
            fs::write(output.join(format!("code-{}.x86", kind + index)), bytes).unwrap();
        }
    }
    let live = [
        0x38, 0xc8, 0xbe, 0x0f, 0x40, 0, 0, 0x0f, 0x90, 6, 0x0f, 0x90, 0xc3, 0x8a, 6, 0xeb, 0,
        0x0f, 0x0b,
    ];
    fs::write(output.join("live.x86"), live).unwrap();
    for truth in [false, true] {
        for same in [false, true] {
            let value = u8::from(truth);
            let original = if same { value } else { value ^ 1 };
            let bytes = [
                0x38, 0xc8, 0xbe, 0x0b, 0xa0, 0, 0, 0x0f, 0x94, 6, 0xb8, original, 0, 0x34, 0x12,
                0xeb, 0, 0x0f, 0x0b,
            ];
            fs::write(
                output.join(format!("smc-{}-{}.x86", u8::from(truth), u8::from(same))),
                bytes,
            )
            .unwrap();
        }
    }
}

fn condition_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0x1600, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    for (offset, value) in [
        (8, 0x1200),
        (12, 0),
        (56, 0x6000),
        (104, 0x3100),
        (108, 40),
        (136, 0x5000),
        (140, 24),
        (192, 0x3150),
        (196, 8),
    ] {
        pe::put32(&mut bytes, pe::OPTIONAL + offset, value);
    }
    let program: &[u8] = &[
        0xb8, 0xff, 0x7f, 0x34, 0x12, 0xb9, 0x80, 0xff, 0x45, 0x23, 0xbb, 0xa5, 0xa5, 0x67, 0x45,
        0xba, 0xa5, 0xa5, 0x56, 0x34, 0xbe, 0xe0, 0x3f, 0x40, 0, 0x38, 0xc8, 0x0f, 0x92, 0x3e,
        0x0f, 0x90, 0x66, 1, 0x0f, 0x99, 0x46, 2, 0x0f, 0x9b, 0x46, 3, 0x72, 0x56, 0x70, 0x54,
        0x38, 0xcc, 0x0f, 0x92, 0x46, 4, 0x0f, 0x90, 0x46, 5, 0x0f, 0x98, 0x46, 6, 0x0f, 0x9a,
        0x46, 7, 0x79, 0x40, 0x7b, 0x3e, 0x38, 0xc9, 0x0f, 0x94, 0x44, 0x3e, 8, 0x0f, 0x95, 5,
        0xe9, 0x3f, 0x40, 0, 0x0f, 0x9c, 0x46, 10, 0x0f, 0x9f, 0x46, 11, 0x8a, 0x46, 8, 0x80, 0xf8,
        1, 0x75, 0x20, 0xb0, 0xff, 0xbf, 0x15, 0, 0xc0, 0xc8, 0x89, 0x3d, 0, 0x30, 0x40, 0, 0x57,
        0xff, 0x15, 0x50, 0x31, 0x40, 0, 0xc7, 5, 4, 0x30, 0x40, 0, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f,
        0x0b, 0xbf, 0xde, 0xc0, 0xad, 0xde, 0x89, 0x3d, 0, 0x30, 0x40, 0, 0x57, 0xff, 0x15, 0x50,
        0x31, 0x40, 0, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 150);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    for index in 0..32 {
        bytes[0x13e0 + index] = index as u8 + 1;
    }
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    for (offset, value) in [(8, 24), (16, 512), (20, 0x1400), (36, 0x4000_0040)] {
        pe::put32(&mut bytes, pe::section(2) + offset, value);
    }
    pe::put32(&mut bytes, 0x1400, 0x1000);
    pe::put32(&mut bytes, 0x1404, 24);
    for (index, offset) in [21, 78, 107, 114, 120, 137, 144].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn memory_set_byte_preserves_conditions_through_write_faults_smc_and_copied_pe() {
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
    let output = root.join("target/p2-memory-set-byte-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    authored_inputs(&output);
    fs::write(output.join("conditions.exe"), condition_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-set-byte/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory SETcc integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
