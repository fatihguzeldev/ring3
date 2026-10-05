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
fn actual_wasm_rotates_registers_once_and_preserves_unaffected_flags() {
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
    let output = root.join("target/p2-rotate-one-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    for (kind, extension) in [("left", 0), ("right", 1)] {
        let mut bytes = [0xcc; 130];
        let specs: Vec<_> = (0..8)
            .map(|destination| {
                let offset = destination * 13;
                bytes[offset..offset + 13].copy_from_slice(&[
                    0x83,
                    0xc3,
                    0x01,
                    0xd1,
                    0xc0 | u8::try_from(extension << 3 | destination).unwrap(),
                    0x0f,
                    0x92,
                    0xc0,
                    0x0f,
                    0x90,
                    0xc2,
                    0xeb,
                    u8::try_from(0x80 - offset - 13).unwrap(),
                ]);
                BlockSpec {
                    entry: GuestAddress(0x1000 + u32::try_from(offset).unwrap()),
                    byte_length: 13,
                }
            })
            .collect();
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let compiled = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        assert_eq!(compiled.metadata().blocks, 8);
        assert_eq!(compiled.metadata().instructions, 40);
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-{kind}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-rotate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual count-one rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
