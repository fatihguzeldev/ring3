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
    for (name, extension) in [("shl", 4), ("shr", 5), ("sar", 7)] {
        canonical.extend_from_slice(&[0xd0, extension << 3 | 6]);
        for address in addresses {
            aliases.extend_from_slice(&[0xd0, address[0] | extension << 3]);
            aliases.extend_from_slice(&address[1..]);
        }
        fs::write(
            output.join(format!("live-{name}.x86")),
            [
                0xbf,
                0x0f,
                0x40,
                0,
                0,
                0xd0,
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
            ],
        )
        .unwrap();
        fs::write(
            output.join(format!("smc-{name}.x86")),
            [
                0xd0,
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
            ],
        )
        .unwrap();
    }
    canonical.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    aliases.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    fs::write(output.join("canonical.x86"), canonical).unwrap();
    fs::write(output.join("aliases.x86"), aliases).unwrap();
}

fn shift_image() -> Vec<u8> {
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
        (140, 20),
        (192, 0x3150),
        (196, 8),
    ] {
        pe::put32(&mut bytes, pe::OPTIONAL + offset, value);
    }
    let program: &[u8] = &[
        0xbe, 0xfd, 0x3f, 0x40, 0, 0xd0, 0x26, 0x0f, 0x92, 0xc3, 0x0f, 0x90, 0xc7, 0x71, 0x34,
        0xd0, 0x6e, 1, 0x0f, 0x92, 0xc2, 0x0f, 0x90, 0xc6, 0x71, 0x29, 0xd0, 0x7e, 2, 0x0f, 0x92,
        0xc1, 0x0f, 0x90, 0xc5, 0x70, 0x1e, 0xb8, 42, 0, 0, 0, 0x89, 0x05, 0, 0x30, 0x40, 0, 0x50,
        0xff, 0x15, 0x50, 0x31, 0x40, 0, 0xc7, 0x05, 4, 0x30, 0x40, 0, 0xa5, 0xa5, 0xa5, 0xa5,
        0x0f, 0x0b, 0xb8, 0xde, 0xc0, 0xad, 0xde, 0x89, 0x05, 0, 0x30, 0x40, 0, 0x50, 0xff, 0x15,
        0x50, 0x31, 0x40, 0, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 87);
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 4096);
    pe::put32(&mut bytes, pe::section(1) + 16, 4096);
    bytes[pe::DATA_RAW..0x1400].fill(0);
    pe::put32(&mut bytes, 0x400, 0x1111_1111);
    pe::put32(&mut bytes, 0x404, 0x2222_2222);
    for index in 0..29 {
        bytes[0x13e0 + index] = index as u8 + 1;
    }
    bytes[0x13fd..0x1400].copy_from_slice(&[0x80, 0x80, 0x81]);
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    for (offset, value) in [(8, 20), (16, 512), (20, 0x1400), (36, 0x4000_0040)] {
        pe::put32(&mut bytes, pe::section(2) + offset, value);
    }
    pe::put32(&mut bytes, 0x1400, 0x1000);
    pe::put32(&mut bytes, 0x1404, 20);
    for (index, offset) in [1, 44, 51, 57, 74, 81].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x1408 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn count_one_memory_byte_shifts_publish_only_after_checked_store_in_actual_wasm() {
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
        .join("target/p2-memory-byte-shift-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    authored_inputs(&output);
    fs::write(output.join("shift.exe"), shift_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-memory-byte-shift/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual memory byte shift integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
