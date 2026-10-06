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
fn actual_wasm_exchanges_old_byte_aliases_and_preserves_full_state() {
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
    let output = root.join("target/p2-xchg8-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for (bank, (first, end)) in [(0u8, 28u8), (28, 56), (56, 64)].into_iter().enumerate() {
        let mut bytes = Vec::new();
        for pair in first..end {
            let left = pair / 8;
            let right = pair % 8;
            let operand = 0xc0 | (right << 3) | left;
            for _ in 0..2 {
                bytes.extend_from_slice(&[0x86, operand]);
            }
        }
        bytes.extend_from_slice(&[0xeb, 0]);
        let extent = [114u32, 114, 34][bank];
        assert_eq!(bytes.len(), extent as usize);
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        assert_eq!(bytes.len(), [116, 116, 36][bank]);
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
            limits,
        )
        .unwrap();
        assert_eq!(compiled.metadata().blocks, 1);
        assert_eq!(compiled.metadata().instructions, [57, 57, 17][bank]);
        let wasm = compiled.wasm_bytes(&memory).unwrap();
        assert!(wasm.len() <= limits.wasm_bytes);
        fs::write(output.join(format!("bank-{bank}.x86")), bytes).unwrap();
        fs::write(output.join(format!("standalone-bank-{bank}.wasm")), wasm).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-xchg8/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual register byte exchange integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
