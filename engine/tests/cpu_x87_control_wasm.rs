use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_no_wait_x87_environment_and_status_preserve_raw_registers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the actual engine Wasm first");
    let expected =
        std::env::var("RING3_ENGINE_SHA256").expect("provide the captured current engine SHA-256");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    let output = root.join("target/p2-x87-control-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    for (name, bytes) in [
        ("x87", &[0xdf, 0xe0, 0xdb, 0xe3, 0xdf, 0xe0, 0xeb, 0][..]),
        ("integer", &[0x90, 0xeb, 0][..]),
    ] {
        let mut source = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
        source.map(0x1000, 1, 7).unwrap();
        source.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        source.upload(0x1000, bytes.len() as u32).unwrap();
        for entries in [false, true] {
            let unit = if entries {
                compile_entry_region(
                    source.memory().unwrap(),
                    &[GuestAddress(0x1000)],
                    CompileLimits::default(),
                )
            } else {
                compile_region(
                    source.memory().unwrap(),
                    &[BlockSpec {
                        entry: GuestAddress(0x1000),
                        byte_length: bytes.len() as u32,
                    }],
                    CompileLimits::default(),
                )
            }
            .unwrap();
            fs::write(
                output.join(format!("standalone-{name}-{entries}.wasm")),
                unit.wasm_bytes(source.memory().unwrap()).unwrap(),
            )
            .unwrap();
        }
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-x87-control/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual x87 integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
