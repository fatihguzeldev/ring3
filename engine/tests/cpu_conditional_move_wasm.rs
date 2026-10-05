use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn actual_wasm_selects_full_registers_and_preserves_condition_flags() {
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
        .join("target/p2-conditional-move-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let mut memory = AddressSpace::new(3).unwrap();
    for (kind, pc) in [("main", 0x1000), ("alias", 0x2000), ("live", 0x4000)] {
        let bytes = if kind == "live" {
            vec![
                0x39, 0xc8, 0x0f, 0x42, 0xd3, 0x0f, 0x43, 0xf7, 0x0f, 0x92, 0xc4, 0x73, 2, 0xeb, 0,
                0x0f, 0x0b,
            ]
        } else {
            let mut bytes = Vec::new();
            for condition in 0_u8..16 {
                let destination = condition % 8;
                let source = if kind == "alias" {
                    destination
                } else {
                    (condition + 3) % 8
                };
                bytes.extend([0x0f, 0x40 + condition, 0xc0 | destination << 3 | source]);
            }
            bytes.extend([0xeb, 0, 0x0f, 0x0b]);
            bytes
        };
        assert_eq!(bytes.len(), if kind == "live" { 17 } else { 52 });
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), &bytes).unwrap();
        let entries = if kind == "live" {
            vec![GuestAddress(pc), GuestAddress(pc + 13)]
        } else {
            vec![GuestAddress(pc)]
        };
        let specs = if kind == "live" {
            vec![
                BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 13,
                },
                BlockSpec {
                    entry: GuestAddress(pc + 13),
                    byte_length: 2,
                },
            ]
        } else {
            vec![BlockSpec {
                entry: GuestAddress(pc),
                byte_length: 50,
            }]
        };
        let compiled = if kind == "alias" {
            compile_entry_region(&memory, &entries, CompileLimits::default()).unwrap()
        } else {
            compile_region(&memory, &specs, CompileLimits::default()).unwrap()
        };
        assert_eq!(
            compiled.metadata().instructions,
            if kind == "live" { 6 } else { 17 }
        );
        fs::write(output.join(format!("{kind}.x86")), &bytes).unwrap();
        fs::write(
            output.join(format!("standalone-{kind}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-conditional-move/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual CMOV integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
