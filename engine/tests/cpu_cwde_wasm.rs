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
fn actual_wasm_sign_extends_accumulator_word_and_preserves_full_state() {
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
    let bank = root.join("target/p2-cwde-fixtures");
    fs::create_dir_all(&bank).unwrap();
    let output = bank.join(unique);
    fs::create_dir(&output).unwrap();
    let bytes = [0x98, 0x98, 0xeb, 0, 0x0f, 0x0b];
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
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(compiled.metadata().instructions, 3);
    fs::write(output.join("cwde.x86"), bytes).unwrap();
    fs::write(
        output.join("standalone.wasm"),
        compiled.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-cwde/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual CWDE integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
