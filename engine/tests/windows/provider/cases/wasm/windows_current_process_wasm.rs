use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    process::EngineInstance,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::support_pe32 as pe;

fn image() -> Vec<u8> {
    let kernel = include_bytes!("../../fixtures/p2-current-process/kernel.text.bin");
    assert_eq!(kernel.len(), 67);
    let caller = [
        0xf9, 0xfd, 0xe8, 0xf9, 0x0f, 0, 0, 0x50, 0xff, 0x15, 0x90, 0x31, 0x40, 0, 0x0f, 0x0b,
    ];
    let mut bytes = pe::image();
    bytes.resize(0x2800, 0);
    pe::put16(&mut bytes, pe::COFF + 18, 0x0102);
    for (field, value) in [(4, 8192), (8, 1536), (12, 0), (56, 0x6000)] {
        pe::put32(&mut bytes, pe::OPTIONAL + field, value);
    }
    for (index, rva, size) in [(1, 0x3100, 40), (5, 0x5000, 28), (12, 0x3180, 24)] {
        pe::put32(&mut bytes, pe::OPTIONAL + 96 + index * 8, rva);
        pe::put32(&mut bytes, pe::OPTIONAL + 100 + index * 8, size);
    }
    for (offset, value) in [(8, 0x1043), (16, 8192)] {
        pe::put32(&mut bytes, pe::section(0) + offset, value);
    }
    bytes[0x200..0x2200].fill(0xcc);
    bytes[0x200..0x210].copy_from_slice(&caller);
    bytes[0x1200..0x1243].copy_from_slice(kernel);
    for (offset, value) in [(8, 1024), (16, 1024), (20, 0x2200)] {
        pe::put32(&mut bytes, pe::section(1) + offset, value);
    }
    bytes[0x2200..0x2600].fill(0);
    pe::put32(&mut bytes, 0x2200, 0xface_cafe);
    pe::put32(&mut bytes, 0x2204, 0x1357_9bdf);
    for (index, value) in [0x3140, 0, 0, 0x31b0, 0x3180].into_iter().enumerate() {
        pe::put32(&mut bytes, 0x2300 + index * 4, value);
    }
    for at in [0x2340, 0x2380] {
        for (index, value) in [0x31c0, 0x31e0, 0x3200, 0x3220, 0x3240]
            .into_iter()
            .enumerate()
        {
            pe::put32(&mut bytes, at + index * 4, value);
        }
    }
    bytes[0x23b0..0x23bd].copy_from_slice(b"KeRnEl32.dLl\0");
    for (at, hint, name) in [
        (0x23c0, 1, b"GetCurrentProcess\0".as_slice()),
        (0x23e0, 2, b"SetLastError\0".as_slice()),
        (0x2400, 3, b"FlushInstructionCache\0".as_slice()),
        (0x2420, 4, b"GetLastError\0".as_slice()),
        (0x2440, 5, b"ExitProcess\0".as_slice()),
    ] {
        pe::put16(&mut bytes, at, hint);
        bytes[at + 2..at + 2 + name.len()].copy_from_slice(name);
    }
    bytes[pe::section(2)..pe::section(2) + 8].copy_from_slice(b".fixups\0");
    for (offset, value) in [(8, 28), (16, 512), (20, 0x2600), (36, 0x4000_0040)] {
        pe::put32(&mut bytes, pe::section(2) + offset, value);
    }
    bytes[0x2600..].fill(0);
    pe::put32(&mut bytes, 0x2600, 0x1000);
    pe::put32(&mut bytes, 0x2604, 12);
    pe::put16(&mut bytes, 0x2608, 0x300a);
    pe::put16(&mut bytes, 0x260a, 0);
    pe::put32(&mut bytes, 0x260c, 0x2000);
    pe::put32(&mut bytes, 0x2610, 16);
    for (index, offset) in [4, 17, 28, 36].into_iter().enumerate() {
        pe::put16(&mut bytes, 0x2614 + index * 2, 0x3000 | offset);
    }
    bytes
}

#[test]
fn compiled_current_process_handle_flows_into_named_flush_and_returns_42() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root
        .join("target/p2-current-process-fixtures")
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
    assert_eq!(ARENA_SIZE, 4612);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    fs::write(output.join("current-process.exe"), image()).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/windows/provider/fixtures/p2-current-process/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node-stdout.txt"), &result.stdout).unwrap();
    fs::write(output.join("node-stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "current-process integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
