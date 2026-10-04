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
    let kinds = [("adc", 0x10, 2), ("sbb", 0x18, 3)];
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
    let mut aliases = Vec::new();
    for (name, opcode, extension) in kinds {
        let mut canonical = Vec::new();
        for source in 0..8 {
            canonical.extend_from_slice(&[opcode, source << 3 | 6]);
        }
        for immediate in [0, 0x7f, 0x80, 0xff] {
            canonical.extend_from_slice(&[0x80, extension << 3 | 6, immediate]);
        }
        canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
        assert_eq!(canonical.len(), 32);
        fs::write(output.join(format!("canonical-{name}.x86")), canonical).unwrap();
        for (source, tail) in shapes {
            assert_eq!(tail[0] & 0x38, 0);
            aliases.extend_from_slice(&[opcode, tail[0] | source << 3]);
            aliases.extend_from_slice(&tail[1..]);
        }
        for immediate in [false, true] {
            let mode = if immediate { "imm" } else { "reg" };
            let mut live = vec![0x80, 0xc1, 1, 0xbf, 0x0f, 0x40, 0, 0];
            if immediate {
                live.extend_from_slice(&[0x80, extension << 3 | 7, 0]);
            } else {
                live.extend_from_slice(&[opcode, 7]);
            }
            live.extend_from_slice(&[0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0xeb, 0, 0x0f, 0x0b]);
            fs::write(output.join(format!("live-{name}-{mode}.x86")), live).unwrap();
            let mut smc = Vec::new();
            if immediate {
                smc.extend_from_slice(&[0x80, extension << 3 | 6, 0]);
            } else {
                smc.extend_from_slice(&[opcode, 6]);
            }
            smc.extend_from_slice(&[0xbf, 0xee, 0xff, 0xc0, 0x51, 0xeb, 0, 0x0f, 0x0b]);
            fs::write(output.join(format!("smc-{name}-{mode}.x86")), smc).unwrap();
        }
    }
    aliases.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    assert_eq!(aliases.len(), 84);
    fs::write(output.join("aliases.x86"), aliases).unwrap();
}

fn carry_image() -> Vec<u8> {
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
        0xb8, 0x00, 0x00, 0x34, 0x12, 0xb9, 0xff, 0xff, 0x45, 0x23, 0xbb, 0xff, 0xff, 0x67, 0x45,
        0xba, 0xff, 0xff, 0x56, 0x34, 0xbe, 0xff, 0x3f, 0x40, 0x00, 0x83, 0xed, 0x01, 0x10, 0x06,
        0x0f, 0x92, 0xc2, 0x83, 0xc5, 0x01, 0x80, 0x16, 0x7f, 0x0f, 0x90, 0xc6, 0x83, 0xed, 0x01,
        0x18, 0x26, 0x0f, 0x90, 0xc3, 0x83, 0xc5, 0x01, 0x80, 0x1e, 0xff, 0x0f, 0x92, 0xc7, 0x0f,
        0x98, 0xc1, 0x0f, 0x9a, 0xc5, 0x81, 0xfa, 0x01, 0x01, 0x56, 0x34, 0x75, 0x38, 0x81, 0xfb,
        0x01, 0x01, 0x67, 0x45, 0x75, 0x30, 0x81, 0xf9, 0x00, 0x00, 0x45, 0x23, 0x75, 0x28, 0x80,
        0x3e, 0x7f, 0x75, 0x23, 0xf6, 0x06, 0x01, 0x74, 0x1e, 0xbe, 0x10, 0x00, 0xc0, 0xc8, 0x89,
        0x35, 0x00, 0x30, 0x40, 0x00, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0xc7, 0x05, 0x04,
        0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b, 0xbe, 0xde, 0xc0, 0xad, 0xde, 0x89,
        0x35, 0x00, 0x30, 0x40, 0x00, 0x56, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 149);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    for index in 0..31 {
        bytes[0x13e0 + index] = index as u8 + 1;
    }
    bytes[0x13ff] = 0xff;
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
    for (index, offset) in [21, 106, 113, 119, 136, 143].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn memory_byte_carry_uses_original_cf_across_real_store_faults_and_copied_pe_paths() {
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
        .join("target/p2-memory-byte-carry-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    authored_inputs(&output);
    fs::write(output.join("carry.exe"), carry_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-carry/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory byte carry integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
