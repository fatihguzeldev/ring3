use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn retirement_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0xa00, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 0x600);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3100);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    pe::put32(&mut bytes, pe::OPTIONAL + 136, 0x5000);
    pe::put32(&mut bytes, pe::OPTIONAL + 140, 20);
    pe::put32(&mut bytes, pe::OPTIONAL + 192, 0x3150);
    pe::put32(&mut bytes, pe::OPTIONAL + 196, 8);
    // inc counter, same-byte code store, resumed mov/cmp/jl, named ExitProcess.
    let program: &[u8] = &[
        0xff, 0x05, 0x00, 0x30, 0x40, 0x00, 0xc7, 0x05, 0x11, 0x10, 0x40, 0x00, 0x06, 0x00, 0xc0,
        0xc8, 0xb8, 0x06, 0x00, 0xc0, 0xc8, 0x83, 0x3d, 0x00, 0x30, 0x40, 0x00, 0x09, 0x7c, 0xe2,
        0x89, 0x05, 0x04, 0x30, 0x40, 0x00, 0x50, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00, 0xc7, 0x05,
        0x08, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b,
    ];
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    pe::put32(&mut bytes, pe::section(0) + 36, 0xe000_0020);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 1024);
    pe::put32(&mut bytes, pe::section(1) + 16, 1024);
    bytes[pe::DATA_RAW..0x800].fill(0);
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    pe::put32(&mut bytes, 0x540, 0x3170);
    pe::put32(&mut bytes, 0x550, 0x3170);
    bytes[0x560..0x56d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x572..0x57e].copy_from_slice(b"ExitProcess\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".keep\0\0\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 512);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0x800);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x6000_0040);
    pe::put32(&mut bytes, 0x800, 0x1000);
    pe::put32(&mut bytes, 0x804, 20);
    for (index, offset) in [2, 8, 23, 32, 39, 45].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x808 + index * 2, 0x3000 | offset);
    }
    // independently current keeper shares only the relocation page, never the writable .text page.
    bytes[0x900..0x905].copy_from_slice(&[0x90, 0xeb, 0x00, 0x0f, 0x0b]);
    bytes
}

#[test]
fn copied_pe_dispatches_through_nine_explicit_stale_retirements_and_same_slot_reinstalls() {
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
        .join("target/p2-resident-retirement-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("retirement.exe"), retirement_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-resident-retirement/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual resident retirement integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
