use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

const PROGRAM: [u8; 122] = [
    // four PUSHes and a real named VirtualAlloc IAT CALL: return offset20.
    0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x01, 0x10, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x15,
    0x54, 0x31, 0x40, 0x00,
    // two guest page stores/loads, three PUSHes and VirtualFree: return60.
    0x89, 0xc7, 0xc7, 0x07, 0xdf, 0x9b, 0x57, 0x13, 0x8b, 0x1f, 0xc7, 0x87, 0x00, 0x10, 0x00, 0x00,
    0xe0, 0xac, 0x68, 0x24, 0x8b, 0xaf, 0x00, 0x10, 0x00, 0x00, 0x68, 0x00, 0x80, 0x00, 0x00, 0x6a,
    0x00, 0x57, 0xff, 0x15, 0x50, 0x31, 0x40, 0x00,
    // reallocate through the same named IAT entry: return80.
    0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x68, 0x01, 0x10, 0x00, 0x00, 0x6a, 0x00, 0xff, 0x15,
    0x54, 0x31, 0x40, 0x00,
    // genuine zero loads and observer stores, then named ExitProcess42: return110.
    0x89, 0xc6, 0x8b, 0x16, 0x8b, 0x9e, 0x00, 0x10, 0x00, 0x00, 0x89, 0x15, 0x00, 0x30, 0x40, 0x00,
    0x89, 0x1d, 0x04, 0x30, 0x40, 0x00, 0x6a, 0x2a, 0xff, 0x15, 0x58, 0x31, 0x40, 0x00,
    // terminal completion must prevent this after-call canary from executing.
    0xc7, 0x05, 0x08, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5, 0xa5, 0x0f, 0x0b,
];

fn release_image() -> Vec<u8> {
    let mut bytes = pe::image();
    bytes.resize(0x800, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 1024);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    for (index, rva, size) in [(1, 0x3100, 40), (5, 0x5000, 24), (12, 0x3150, 16)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    pe::put32(&mut bytes, pe::section(0) + 8, PROGRAM.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + PROGRAM.len()].copy_from_slice(&PROGRAM);
    pe::put32(&mut bytes, pe::section(1) + 8, 512);
    bytes[pe::DATA_RAW..pe::DATA_RAW + 512].fill(0);
    pe::put32(&mut bytes, pe::DATA_RAW, 0x1111_1111);
    pe::put32(&mut bytes, pe::DATA_RAW + 4, 0x2222_2222);
    pe::put32(&mut bytes, pe::DATA_RAW + 16, 0xface_cafe);
    for (index, value) in [0x3140, 0, 0, 0x3160, 0x3150].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    for table in [0x540, 0x550] {
        for (index, name) in [0x3190, 0x31b0, 0x3170].into_iter().enumerate() {
            pe::put32(&mut bytes, table + index * 4, name);
        }
    }
    bytes[0x560..0x56d].copy_from_slice(b"KeRnEl32.dLl\0");
    for (at, hint, symbol) in [
        (0x570, 0x1234, b"ExitProcess\0".as_slice()),
        (0x590, 0x5678, b"VirtualFree\0".as_slice()),
        (0x5b0, 0x9abc, b"VirtualAlloc\0".as_slice()),
    ] {
        pe::put16(&mut bytes, at, hint);
        bytes[at + 2..at + 2 + symbol.len()].copy_from_slice(symbol);
    }
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    pe::put32(&mut bytes, pe::section(2) + 8, 24);
    pe::put32(&mut bytes, pe::section(2) + 16, 512);
    pe::put32(&mut bytes, pe::section(2) + 20, 0x600);
    pe::put32(&mut bytes, pe::section(2) + 36, 0x4000_0040);
    pe::put32(&mut bytes, 0x600, 0x1000);
    pe::put32(&mut bytes, 0x604, 24);
    for (index, offset) in [16, 56, 76, 92, 98, 106, 112].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x608 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn copied_named_virtual_free_reuses_zeroed_pages_after_rust_startup_in_both_owners() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
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
        .join("target/p2-pe32-virtual-free-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("release.exe"), release_image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-pe32-virtual-free/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual named virtual release integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
