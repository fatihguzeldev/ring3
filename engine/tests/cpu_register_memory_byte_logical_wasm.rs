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
    let kinds = [("and", 0x22), ("or", 0x0a), ("xor", 0x32)];
    let shapes: &[(u8, &[u8])] = &[
        (0, &[0x00]),
        (4, &[0x04, 0x45, 1, 0, 0, 0]),
        (1, &[0x41, 0xf0]),
        (5, &[0x04, 0x4d, 1, 0, 0, 0]),
        (2, &[0x44, 0x54, 0x11]),
        (6, &[0x44, 0x95, 0]),
        (3, &[0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff]),
        (7, &[0x83, 0, 1, 0, 0]),
    ];
    let mut canonical = Vec::new();
    let mut aliases = Vec::new();
    for (name, opcode) in kinds {
        for destination in 0..8 {
            canonical.extend_from_slice(&[opcode, destination << 3 | 6]);
        }
        for (destination, tail) in shapes {
            assert_eq!(tail[0] & 0x38, 0);
            aliases.extend_from_slice(&[opcode, tail[0] | destination << 3]);
            aliases.extend_from_slice(&tail[1..]);
        }
        let live = [
            0xbe, 0x0f, 0x40, 0, 0, opcode, 6, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0, 0x0f,
            0x0b,
        ];
        fs::write(output.join(format!("live-{name}.x86")), live).unwrap();
    }
    canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    aliases.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    assert_eq!(canonical.len(), 52);
    assert_eq!(aliases.len(), 124);
    fs::write(output.join("canonical.x86"), canonical).unwrap();
    fs::write(output.join("aliases.x86"), aliases).unwrap();
}

fn logical_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0x1600, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 0x1200);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3100);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    pe::put32(&mut bytes, pe::OPTIONAL + 136, 0x5000);
    pe::put32(&mut bytes, pe::OPTIONAL + 140, 24);
    pe::put32(&mut bytes, pe::OPTIONAL + 192, 0x3150);
    pe::put32(&mut bytes, pe::OPTIONAL + 196, 8);
    let program: &[u8] = &[
        0xb8, 0xff, 0x3f, 0x40, 0x00, 0xb9, 0x80, 0x0f, 0x45, 0x23, 0xbb, 0xff, 0xff, 0x67, 0x45,
        0xba, 0xff, 0xff, 0x56, 0x34, 0x80, 0xc1,
        0x80, // sets carry and overflow before the first logical read
        0x22, 0x00, // reads through the original destination parent before insertion
        0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0xbe, 0xfe, 0x3f, 0x40, 0x00, 0x0a, 0x2e, 0x0f, 0x98,
        0xc3, 0xb1, 0x80, 0x32, 0x0e, 0x0f, 0x94, 0xc7, 0x22, 0x26, 0x0f, 0x9a, 0xc2, 0x0a, 0x06,
        0x32, 0x26, 0x81, 0xfa, 0x01, 0x00, 0x56, 0x34, 0x75, 0x39, 0x81, 0xfb, 0x01, 0x01, 0x67,
        0x45, 0x75, 0x31, 0x81, 0xf9, 0x00, 0x8f, 0x45, 0x23, 0x75, 0x29, 0x80, 0x3e, 0x80, 0x75,
        0x24, 0xf6, 0x46, 0x01, 0x0f, 0x74, 0x1e, 0xbe, 0x13, 0x00, 0xc0, 0xc8, 0x89, 0x35, 0x00,
        0x30, 0x40, 0x00, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0xc7, 0x05, 0x04, 0x30, 0x40,
        0x00, 0xa5, 0xa5, 0xa5,
        0xa5, // terminal completion must keep the after-return canary untouched
        0x0f, 0x0b, 0xbe, 0xde, 0xc0, 0xad, 0xde, 0x89, 0x35, 0x00, 0x30, 0x40, 0x00, 0x56, 0xff,
        0x15, 0x50, 0x31, 0x40, 0x00, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 142);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    for index in 0..30 {
        bytes[0x13e0 + index] = index as u8 + 1;
    }
    bytes[0x13fe] = 0x80;
    bytes[0x13ff] = 0x0f;
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 24);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0x1400);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, 0x1400, 0x1000);
    pe::put32(&mut bytes, 0x1404, 24);
    for (index, offset) in [1, 32, 99, 106, 112, 129, 136].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}
#[test]
fn register_memory_byte_logical_reads_before_parent_write_and_copied_pe_paths() {
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
        .join("target/p2-register-memory-byte-logical-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    authored_inputs(&output);
    fs::write(output.join("logical.exe"), logical_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-register-memory-byte-logical/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual register memory byte logical integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
