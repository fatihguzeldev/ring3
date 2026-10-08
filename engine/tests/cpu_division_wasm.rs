use ring3_engine::{
    abi::arena::ARENA_SIZE,
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_wasm_division_has_precise_faults_aliases_and_version_five_continuations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(
        engine.is_file(),
        "build the integrated actual engine wasm first"
    );
    let expected = std::env::var("RING3_ENGINE_SHA256")
        .expect("pin the integrated actual engine with RING3_ENGINE_SHA256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "RING3_ENGINE_SHA256 must be a lowercase SHA-256"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-division-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 1).unwrap().arena(),
    )
    .unwrap();
    for (kind, base) in [("unsigned", 0xf0), ("signed", 0xf8)] {
        let mut bytes = vec![0xcc; 128];
        let specs: Vec<_> = (0..8)
            .map(|source| {
                let offset = source * 16;
                bytes[offset..offset + 7].copy_from_slice(&[
                    0xf7,
                    base + source as u8,
                    0xe9,
                    0,
                    0,
                    0,
                    0,
                ]);
                bytes[offset + 3..offset + 7]
                    .copy_from_slice(&(0x1800_i32 - (0x1000 + offset as i32 + 7)).to_le_bytes());
                BlockSpec {
                    entry: GuestAddress(0x1000 + offset as u32),
                    byte_length: 7,
                }
            })
            .collect();
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        let unit = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        fs::write(output.join(format!("{kind}.x86")), bytes).unwrap();
        fs::write(
            output.join(format!("standalone-{kind}.wasm")),
            unit.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
    let chain = [
        0xb9, 5, 0, 0, 0, 0xba, 0, 0, 0, 0, 0xf7, 0xf1, 0xf7, 0xf8, 0xeb, 0,
    ];
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1400), &chain).unwrap();
    let unit = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1400),
            byte_length: chain.len() as u32,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    fs::write(output.join("chain.x86"), chain).unwrap();
    fs::write(
        output.join("standalone-chain.wasm"),
        unit.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-division/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .arg(ARENA_SIZE.to_string())
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "division integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
