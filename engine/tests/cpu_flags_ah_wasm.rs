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
fn actual_wasm_transfers_status_flags_through_ah_and_carry_consumers() {
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
    let output = root.join("target/p2-flags-ah-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let lahf = [0x9f, 0x9f, 0xeb, 0, 0x0f, 0x0b];
    let sahf = [0x9e, 0x9f, 0x9e, 0xeb, 0, 0x0f, 0x0b];
    let live = [0x9e, 0x0f, 0x42, 0xca, 0x83, 0xd3, 0, 0xeb, 0, 0x0f, 0x0b];
    for (name, bytes, instructions) in [
        ("lahf", lahf.as_slice(), 3),
        ("sahf", sahf.as_slice(), 4),
        ("live", live.as_slice(), 4),
    ] {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let compiled = compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: u32::try_from(bytes.len() - 2).unwrap(),
            }],
            CompileLimits::default(),
        )
        .unwrap();
        assert_eq!(compiled.metadata().instructions, instructions);
        fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-{name}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-flags-ah/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual FLAGS/AH integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
