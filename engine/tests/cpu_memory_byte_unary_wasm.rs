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
    let kinds = [
        ("inc", 0xfe, 0),
        ("dec", 0xfe, 1),
        ("neg", 0xf6, 3),
        ("not", 0xf6, 2),
    ];
    let mut canonical = Vec::new();
    let mut aliases = Vec::new();
    let addresses: &[&[u8]] = &[
        &[0x00],
        &[0x41, 0xf0],
        &[0x44, 0x4a, 0x11],
        &[0x84, 0xf3, 0xe0, 0xff, 0xff, 0xff],
        &[0x04, 0x24],
        &[0x45, 0],
        &[0x06],
        &[0x87, 0, 1, 0, 0],
    ];
    for (name, opcode, extension) in kinds {
        canonical.extend_from_slice(&[opcode, extension << 3 | 6]);
        for address in addresses {
            aliases.push(opcode);
            aliases.push(address[0] | extension << 3);
            aliases.extend_from_slice(&address[1..]);
        }
        let live = [
            0xbf,
            0x0f,
            0x40,
            0,
            0,
            opcode,
            extension << 3 | 7,
            0x0f,
            0x92,
            0xc3,
            0x0f,
            0x90,
            0xc7,
            0xeb,
            0,
            0x0f,
            0x0b,
        ];
        fs::write(output.join(format!("live-{name}.x86")), live).unwrap();
        let smc = [
            opcode,
            extension << 3 | 6,
            0xbf,
            0xee,
            0xff,
            0xc0,
            0x51,
            0xeb,
            0,
            0x0f,
            0x0b,
        ];
        fs::write(output.join(format!("smc-{name}.x86")), smc).unwrap();
    }
    canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    aliases.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    fs::write(output.join("canonical.x86"), canonical).unwrap();
    fs::write(output.join("aliases.x86"), aliases).unwrap();
}

fn unary_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0x1600, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 0x1200);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3100);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    pe::put32(&mut bytes, pe::OPTIONAL + 136, 0x5000);
    pe::put32(&mut bytes, pe::OPTIONAL + 140, 20);
    pe::put32(&mut bytes, pe::OPTIONAL + 192, 0x3150);
    pe::put32(&mut bytes, pe::OPTIONAL + 196, 8);
    let program: &[u8] = &[
        0xb8, 0xff, 0x01, 0x34, 0x12, 0xbe, 0xff, 0x3f, 0x40, 0, 0x04, 0x01, 0xfe, 0x06, 0x0f,
        0x92, 0xc3, 0x71, 0x41, 0xfe, 0x0e, 0x0f, 0x92, 0xc7, 0xf6, 0x16, 0x71, 0x38, 0xf6, 0x06,
        0x80, 0x79, 0x33, 0xf6, 0x1e, 0x0f, 0x90, 0xc2, 0x73, 0x2c, 0x80, 0x3e, 0x80, 0x0f, 0x94,
        0xc6, 0x81, 0xfa, 0x01, 0x01, 0, 0, 0x75, 0x1e, 0xbe, 0x0d, 0, 0xc0, 0xc8, 0x89, 0x35, 0,
        0x30, 0x40, 0, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0, 0xc7, 0x05, 0x04, 0x30, 0x40, 0,
        0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b, 0xbe, 0xde, 0xc0, 0xad, 0xde, 0x89, 0x35, 0, 0x30,
        0x40, 0, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 104);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    for index in 0..31 {
        bytes[0x13e0 + index] = index as u8 + 1;
    }
    bytes[0x13ff] = 0x7f;
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 20);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0x1400);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, 0x1400, 0x1000);
    pe::put32(&mut bytes, 0x1404, 20);
    for (index, offset) in [6, 61, 68, 74, 91, 98].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn memory_byte_unary_writes_exactly_one_byte_and_commits_only_after_checked_store() {
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
        .join("target/p2-memory-byte-unary-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    authored_inputs(&output);
    fs::write(output.join("unary.exe"), unary_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-unary/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory byte unary integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
