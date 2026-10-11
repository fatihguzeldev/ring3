use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    process::EngineInstance,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[path = "support/pe32.rs"]
#[allow(dead_code)]
mod pe;

fn image() -> Vec<u8> {
    let program: &[u8] = &[
        0xf9, 0xfd, 0x6a, 0x04, 0x68, 0x00, 0x30, 0x00, 0x00, 0x6a, 0x40, 0x6a, 0x00, 0xff, 0x15,
        0x80, 0x31, 0x40, 0x00, 0x89, 0xc7, 0xc7, 0x07, 0xb8, 0x2a, 0x00, 0x00, 0xc7, 0x47, 0x04,
        0x00, 0xc3, 0xcc, 0xcc, 0x68, 0x00, 0x30, 0x40, 0x00, 0x6a, 0x20, 0x6a, 0x06, 0x57, 0xff,
        0x15, 0x84, 0x31, 0x40, 0x00, 0x6a, 0x06, 0x57, 0x6a, 0xff, 0xff, 0x15, 0x88, 0x31, 0x40,
        0x00, 0xff, 0xd7, 0x89, 0xc3, 0x6a, 0x06, 0x57, 0x6a, 0xff, 0xff, 0x15, 0x88, 0x31, 0x40,
        0x00, 0xff, 0xd7, 0x89, 0xc6, 0x68, 0x00, 0x30, 0x40, 0x00, 0x6a, 0x40, 0x6a, 0x06, 0x57,
        0xff, 0x15, 0x84, 0x31, 0x40, 0x00, 0x6a, 0x06, 0x57, 0x6a, 0xff, 0xff, 0x15, 0x88, 0x31,
        0x40, 0x00, 0xc7, 0x07, 0xb8, 0x2a, 0x00, 0x00, 0x6a, 0x06, 0x57, 0x6a, 0xff, 0xff, 0x15,
        0x88, 0x31, 0x40, 0x00, 0x68, 0x00, 0x80, 0x00, 0x00, 0x6a, 0x00, 0x57, 0xff, 0x15, 0x8c,
        0x31, 0x40, 0x00, 0x6a, 0x06, 0x57, 0x6a, 0xff, 0xff, 0x15, 0x88, 0x31, 0x40, 0x00, 0x68,
        0xff, 0xff, 0xff, 0xff, 0x6a, 0x00, 0x6a, 0xff, 0xff, 0x15, 0x88, 0x31, 0x40, 0x00, 0x53,
        0xff, 0x15, 0x90, 0x31, 0x40, 0x00, 0xc7, 0x05, 0x0c, 0x30, 0x40, 0x00, 0xa5, 0xa5, 0xa5,
        0xa5, 0x0f, 0x0b,
    ];
    assert_eq!(program.len(), 183);
    let mut bytes = pe::image();
    bytes.resize(0xa00, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    pe::put32(&mut bytes, pe::OPTIONAL + 8, 1536);
    pe::put32(&mut bytes, pe::OPTIONAL + 12, 0);
    pe::put32(&mut bytes, pe::OPTIONAL + 56, 0x6000);
    for (index, rva, size) in [(1, 0x3100, 40), (5, 0x5000, 36), (12, 0x3180, 24)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    pe::put32(&mut bytes, pe::section(0) + 8, program.len() as u32);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + 512].fill(0xcc);
    bytes[pe::TEXT_RAW..pe::TEXT_RAW + program.len()].copy_from_slice(program);
    pe::put32(&mut bytes, pe::section(1) + 8, 1024);
    pe::put32(&mut bytes, pe::section(1) + 16, 1024);
    bytes[pe::DATA_RAW..pe::DATA_RAW + 1024].fill(0);
    pe::put32(&mut bytes, pe::DATA_RAW, 0xa5a5_a5a5);
    pe::put32(&mut bytes, pe::DATA_RAW + 12, 0xface_cafe);
    for (index, value) in [0x3140, 0, 0, 0x31b0, 0x3180].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x500 + index * 4, value);
    }
    for at in [0x540, 0x580] {
        for (index, name) in [0x31c0, 0x31e0, 0x3200, 0x3220, 0x3240]
            .into_iter()
            .enumerate()
        {
            pe::put32(&mut bytes, at + index * 4, name);
        }
    }
    bytes[0x5b0..0x5bd].copy_from_slice(b"KeRnEl32.dLl\0");
    for (at, hint, symbol) in [
        (0x5c0, 1, b"VirtualAlloc\0".as_slice()),
        (0x5e0, 2, b"VirtualProtect\0".as_slice()),
        (0x600, 3, b"FlushInstructionCache\0".as_slice()),
        (0x620, 4, b"VirtualFree\0".as_slice()),
        (0x640, 5, b"ExitProcess\0".as_slice()),
    ] {
        pe::put16(&mut bytes, at, hint);
        bytes[at + 2..at + 2 + symbol.len()].copy_from_slice(symbol);
    }
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    for (offset, value) in [(8, 36), (16, 512), (20, 0x800), (36, 0x4000_0040)] {
        pe::put32(&mut bytes, pe::section(2) + offset, value);
    }
    pe::put32(&mut bytes, 0x800, 0x1000);
    pe::put32(&mut bytes, 0x804, 36);
    for (index, offset) in [
        15, 35, 46, 57, 72, 81, 92, 103, 120, 134, 145, 160, 167, 173,
    ]
    .into_iter()
    .enumerate()
    {
        pe::put16(&mut bytes, 0x808 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn named_flush_preserves_generated_code_and_existing_invalidation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root
        .join("target/p2-instruction-cache-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(ARENA_SIZE, 4364);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    fs::write(output.join("instruction-cache.exe"), image()).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-instruction-cache/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    fs::write(output.join("node-stdout.txt"), &result.stdout).unwrap();
    fs::write(output.join("node-stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "instruction cache integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
