use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn callback_image(selected: bool) -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0xe00, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 4, 0x600);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 0x600);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3100);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    pe::put32(&mut bytes, pe::OPTIONAL + 136, 0x5000);
    pe::put32(&mut bytes, pe::OPTIONAL + 140, 20);
    pe::put32(&mut bytes, pe::OPTIONAL + 192, 0x3160);
    pe::put32(&mut bytes, pe::OPTIONAL + 196, 20);
    // SetLastError, private callback trigger CALL, restored outer scalar, named ExitProcess.
    let outer: &[u8] = &[
        0x68, 0x67, 0x45, 0x23, 0xf1, 0xff, 0x15, 0x64, 0x31, 0x40, 0x00, 0xe8, 0xf0, 0x00, 0x00,
        0x00, 0x89, 0xc2, 0x89, 0x15, 0x00, 0x30, 0x40, 0x00, 0x52, 0xff, 0x15, 0x68, 0x31, 0x40,
        0x00, 0xc7, 0x05, 0x04, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b,
    ];
    // VA4097, two zero loads, checked second-page store/load, GLE, scalar RET.
    let body: &[u8] = &[
        0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x01, 0x10, 0x00, 0x00, 0x6a, 0x00, 0xff,
        0x15, 0x6c, 0x31, 0x40, 0x00, 0x89, 0xc7, 0x8b, 0x0f, 0x8b, 0x9f, 0x00, 0x10, 0x00, 0x00,
        0x09, 0xd9, 0xc7, 0x87, 0x00, 0x10, 0x00, 0x00, 0xe0, 0xac, 0x68, 0x24, 0x8b, 0x87, 0x00,
        0x10, 0x00, 0x00, 0x35, 0xe0, 0xac, 0x68, 0x24, 0x09, 0xc8, 0x83, 0xf8, 0x00, 0x0f, 0x85,
        0x0c, 0x00, 0x00, 0x00, 0xff, 0x15, 0x60, 0x31, 0x40, 0x00, 0xb8, 0x05, 0x00, 0xc0, 0xc8,
        0xc3, 0xb8, 0xde, 0xc0, 0xad, 0xde, 0xc3,
    ];
    pe::put32(&mut bytes, pe::section(0) + 8, 0x500);
    pe::put32(&mut bytes, pe::section(0) + 16, 0x600);
    bytes[0x200..0x800].fill(0xcc);
    bytes[0x200..0x200 + outer.len()].copy_from_slice(outer);
    // private generic trigger and immutable callback return, declared as GateSpecs by the host.
    bytes[0x300..0x302].copy_from_slice(&[0x0f, 0x0b]);
    bytes[0x500..0x502].copy_from_slice(&[0x0f, 0x0b]);
    let body_offset = if selected { 0x400 } else { 0x200 };
    if selected {
        bytes[0x400..0x405].copy_from_slice(&[0xe9, 0xfb, 0x01, 0x00, 0x00]);
    }
    bytes[0x200 + body_offset..0x200 + body_offset + body.len()].copy_from_slice(body);
    pe::put32(&mut bytes, pe::section(1) + 8, 1024);
    pe::put32(&mut bytes, pe::section(1) + 16, 1024);
    pe::put32(&mut bytes, pe::section(1) + 20, 0x800);
    bytes[0x800..0xc00].fill(0);
    for (index, value) in [0x3140, 0, 0, 0x3180, 0x3160].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x900 + index * 4, value);
    }
    for (index, hint) in [0x31a0, 0x31c0, 0x31e0, 0x3200].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x940 + index * 4, hint);
        pe::put32(&mut bytes, 0x960 + index * 4, hint);
    }
    bytes[0x980..0x98d].copy_from_slice(b"kernel32.dll\0");
    bytes[0x9a2..0x9af].copy_from_slice(b"GetLastError\0");
    bytes[0x9c2..0x9cf].copy_from_slice(b"SetLastError\0");
    bytes[0x9e2..0x9ee].copy_from_slice(b"ExitProcess\0");
    bytes[0xa02..0xa0f].copy_from_slice(b"VirtualAlloc\0");
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 20);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0xc00);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, 0xc00, 0x1000);
    pe::put32(&mut bytes, 0xc04, 20);
    for (index, offset) in [
        7,
        20,
        27,
        33,
        body_offset as u16 + 16,
        body_offset as u16 + 66,
    ]
    .into_iter()
    .enumerate()
    {
        pe::put16(&mut bytes, 0xc08 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn active_callback_completes_windows_alloc_and_returns_to_the_frozen_outer() {
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
        .join("target/p2-callback-windows-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("home.exe"), callback_image(false)).unwrap();
    fs::write(output.join("selected.exe"), callback_image(true)).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-callback-windows/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual callback Windows integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
