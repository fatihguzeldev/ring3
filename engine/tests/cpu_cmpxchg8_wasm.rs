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
fn actual_wasm_compares_current_al_and_byte_parents_without_owner_replay() {
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
    let output = root.join("target/p2-cmpxchg8-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for destination in 0u8..8 {
        let mut bytes = Vec::new();
        let mut specs = Vec::new();
        for source in 0u8..8 {
            assert_eq!(bytes.len(), usize::from(source) * 14);
            let modrm = 0xc0 | (source << 3) | destination;
            bytes.extend_from_slice(&[0x0f, 0xb0, modrm, 0x0f, 0x94, 0xc0, 0x0f, 0x92, 0xc2]);
            bytes.extend_from_slice(&[0x0f, 0xb0, modrm, 0xeb, 112 - (source + 1) * 14]);
            specs.push(BlockSpec {
                entry: GuestAddress(0x1000 + u32::from(source) * 14),
                byte_length: 14,
            });
        }
        assert_eq!(bytes.len(), 112);
        bytes.extend_from_slice(&[0x0f, 0x0b]);
        assert_eq!(bytes.len(), 114);
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
        assert_eq!(compiled.metadata().instructions, 40);
        let wasm = compiled.wasm_bytes(&memory).unwrap();
        assert!(wasm.len() <= limits.wasm_bytes);
        fs::write(output.join(format!("bank-{destination}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-initial-bank-{destination}.wasm")),
            wasm,
        )
        .unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-cmpxchg8/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual register CMPXCHG8 integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
