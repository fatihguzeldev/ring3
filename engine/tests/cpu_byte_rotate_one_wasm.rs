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
fn actual_wasm_rotates_byte_aliases_once_with_live_partial_register_continuations() {
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
    let output = root.join("target/p2-byte-rotate-one-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for (kind, extension) in [("left", 0), ("right", 8)] {
        let mut bytes = [0xcc; 194];
        let mut specs = Vec::new();
        let mut offset = 0;
        for destination in 0..8 {
            let entry = offset;
            if destination == 0 {
                bytes[offset..offset + 3].copy_from_slice(&[0x83, 0xc3, 1]);
                offset += 3;
            }
            let operand = 0xc0 | extension | u8::try_from(destination).unwrap();
            for _ in 0..2 {
                bytes[offset..offset + 8]
                    .copy_from_slice(&[0xd0, operand, 0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2]);
                offset += 8;
            }
            if destination == 7 {
                bytes[offset..offset + 5].copy_from_slice(&[0xbf, 0xef, 0xbe, 0xad, 0xde]);
                offset += 5;
            }
            bytes[offset] = 0xe9;
            bytes[offset + 1..offset + 5]
                .copy_from_slice(&i32::try_from(192 - offset - 5).unwrap().to_le_bytes());
            offset += 5;
            specs.push(BlockSpec {
                entry: GuestAddress(0x1000 + u32::try_from(entry).unwrap()),
                byte_length: u32::try_from(offset - entry).unwrap(),
            });
        }
        assert_eq!(offset, 176);
        assert_eq!(
            specs
                .iter()
                .map(|spec| spec.entry.0 - 0x1000)
                .collect::<Vec<_>>(),
            [0, 24, 45, 66, 87, 108, 129, 150]
        );
        bytes[192..].copy_from_slice(&[0x0f, 0x0b]);
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let compiled = compile_region(&memory, &specs, limits).unwrap();
        assert_eq!(compiled.metadata().blocks, 8);
        assert_eq!(compiled.metadata().instructions, 58);
        let wasm = compiled.wasm_bytes(&memory).unwrap();
        assert!(wasm.len() <= limits.wasm_bytes);
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
        fs::write(output.join(format!("standalone-{kind}.wasm")), wasm).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-rotate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual count-one byte rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
