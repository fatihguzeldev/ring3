use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn actual_wasm_moves_c6_bytes_and_resumes_current_owners_without_replay() {
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
        .join("target/p2-byte-move-immediate-modrm-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    let mut bytes = Vec::new();
    for alias in 0u8..8 {
        assert_eq!(bytes.len(), usize::from(alias) * 20);
        bytes.extend_from_slice(&[0xc6, 0xc0 | alias, 0, 0xc6, 0xc0 | alias, 0]);
        bytes.extend_from_slice(&[0x88, 0xc0 | (alias << 3) | (alias ^ 4)]);
        for immediate in [0x80, 0xff] {
            for _ in 0..2 {
                bytes.extend_from_slice(&[0xc6, 0xc0 | alias, immediate]);
            }
        }
    }
    assert_eq!(bytes.len(), 160);
    bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
    assert_eq!(bytes.len(), 164);
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 162,
        }],
        limits,
    )
    .unwrap();
    assert_eq!(compiled.metadata().blocks, 1);
    assert_eq!(compiled.metadata().instructions, 57);
    let wasm = compiled.wasm_bytes(&memory).unwrap();
    assert!(wasm.len() <= limits.wasm_bytes);
    fs::write(output.join("bank-0.x86"), bytes).unwrap();
    fs::write(output.join("standalone-initial-bank-0.wasm"), wasm).unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-move-immediate-modrm/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual C6 register byte move integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
