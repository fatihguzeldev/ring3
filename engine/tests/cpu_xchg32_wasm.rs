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
fn actual_wasm_exchanges_register_values_and_preserves_full_state() {
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
    let bank = root.join("target/p2-xchg32-fixtures");
    fs::create_dir_all(&bank).unwrap();
    let output = bank.join(unique);
    fs::create_dir(&output).unwrap();
    for (batch, (first, end)) in [(0u8, 28u8), (28, 56), (56, 64)].into_iter().enumerate() {
        let mut bytes = Vec::new();
        for pair in first..end {
            let left = pair / 8;
            let right = pair % 8;
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x87, 0xc0 | (right << 3) | left]);
            }
        }
        if batch == 2 {
            for opcode in 0x91..=0x97 {
                bytes.extend_from_slice(&[opcode, opcode]);
            }
            bytes.push(0x90);
        }
        bytes.extend_from_slice(&[0xeb, 0, 0x0f, 0x0b]);
        let extent = [114, 114, 49][batch];
        let instructions = [57, 57, 32][batch];
        assert_eq!(bytes.len(), extent as usize + 2);
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
                byte_length: extent,
            }],
            CompileLimits::default(),
        )
        .unwrap();
        assert_eq!(compiled.metadata().instructions, instructions);
        fs::write(output.join(format!("xchg32-batch-{batch}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-batch-{batch}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-xchg32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual XCHG32 integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
