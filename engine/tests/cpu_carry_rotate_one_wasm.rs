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
fn actual_wasm_rotates_registers_and_carry_once_with_live_flag_continuations() {
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
        .join("target/p2-carry-rotate-one-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    for (kind, extension) in [("left", 0x10), ("right", 0x18)] {
        let mut bytes = [0xcc; 130];
        let mut offset = 0;
        let specs: Vec<_> = (0..8)
            .map(|destination| {
                let entry = offset;
                if destination < 3 {
                    bytes[offset] = [0xf8, 0xf9, 0xf5][destination];
                    offset += 1;
                }
                let operand = 0xc0 | extension | u8::try_from(destination).unwrap();
                bytes[offset..offset + 12].copy_from_slice(&[
                    0xd1,
                    operand,
                    0x0f,
                    0x92,
                    0xc0,
                    0x0f,
                    0x90,
                    0xc2,
                    0xd1,
                    operand,
                    0xeb,
                    u8::try_from(128 - offset - 12).unwrap(),
                ]);
                offset += 12;
                BlockSpec {
                    entry: GuestAddress(0x1000 + u32::try_from(entry).unwrap()),
                    byte_length: u32::try_from(offset - entry).unwrap(),
                }
            })
            .collect();
        assert_eq!(offset, 99);
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
        assert_eq!(compiled.metadata().instructions, 43);
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-{kind}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-carry-rotate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual count-one carry rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
