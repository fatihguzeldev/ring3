use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn micro_inputs(output: &Path) {
    let mut canonical = Vec::new();
    for (register_opcode, immediate_opcode, extension) in [(0x38, 0x80, 7), (0x84, 0xf6, 0)] {
        for source in 0..8 {
            canonical.extend_from_slice(&[register_opcode, source << 3 | 6]);
        }
        for immediate in [0, 0x7f, 0x80, 0xff] {
            canonical.extend_from_slice(&[immediate_opcode, extension << 3 | 6, immediate]);
        }
    }
    canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    fs::write(output.join("canonical.x86"), canonical).unwrap();
    let addresses: &[&[u8]] = &[
        &[0x00],
        &[0x60, 0x20],
        &[0x49, 0xf0],
        &[0xa9, 0, 1, 0, 0],
        &[0x54, 0x7a, 0x10],
        &[0x74, 0xba, 0xf8],
        &[0x9c, 0xf3, 0xe0, 0xff, 0xff, 0xff],
        &[0x3c, 0x33],
    ];
    let mut aliases = Vec::new();
    for opcode in [0x38, 0x84] {
        for address in addresses {
            aliases.push(opcode);
            aliases.extend_from_slice(address);
        }
    }
    aliases.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    fs::write(output.join("aliases.x86"), aliases).unwrap();
    // fault paths share one unit and resume from the genuine faulting pc.
    let live = [
        0xbf, 0x00, 0x40, 0, 0, 0xf6, 0x07, 0, 0x0f, 0x94, 0xc0, 0x38, 0x26, 0x0f, 0x90, 0xc3,
        0x38, 0x45, 0, 0x0f, 0x90, 0xc7, 0x84, 0x44, 0x4a, 0x11, 0x0f, 0x94, 0xc2, 0xeb, 0, 0x0f,
        0x0b,
    ];
    assert_eq!(live.len(), 33);
    fs::write(output.join("live.x86"), live).unwrap();
}

fn predicate_image() -> Vec<u8> {
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
        0xb8, 0x7f, 0x01, 0x34, 0x12, 0xbe, 0xff, 0x3f, 0x40, 0, 0x38, 0x26, 0x71, 0x3a, 0x80,
        0x3e, 0x80, 0x75, 0x35, 0x84, 0x06, 0x0f, 0x94, 0xc3, 0x75, 0x2e, 0xf6, 0x06, 0x80, 0x0f,
        0x98, 0xc7, 0x79, 0x26, 0x81, 0xfb, 0x01, 0x01, 0, 0, 0x75, 0x1e, 0xbe, 0x0c, 0, 0xc0,
        0xc8, 0x89, 0x35, 0, 0x30, 0x40, 0, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0, 0xc7, 0x05,
        0x04, 0x30, 0x40, 0, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b, 0xbe, 0xde, 0xc0, 0xad, 0xde,
        0x89, 0x35, 0, 0x30, 0x40, 0, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 92);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    bytes[0x13ff] = 0x80;
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
    for (index, offset) in [6, 49, 56, 62, 79, 86].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn memory_byte_predicates_read_once_preserve_operands_and_resume_real_faults() {
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
        .join("target/p2-memory-byte-predicate-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    micro_inputs(&output);
    fs::write(output.join("predicate.exe"), predicate_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-predicate/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory byte predicate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
