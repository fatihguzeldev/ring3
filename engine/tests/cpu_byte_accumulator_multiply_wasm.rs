use ring3_engine::{abi::arena::ARENA_SIZE, process::EngineInstance};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bank(field: u8) -> [u8; 130] {
    let mut bytes = [0xcc; 130];
    for alias in 0u8..8 {
        let at = usize::from(alias) * 12;
        let operand = 0xc0 | field | alias;
        bytes[at..at + 12].copy_from_slice(&[
            0xf6,
            operand,
            0xf6,
            operand,
            0x0f,
            0x92,
            0xc2,
            0x0f,
            0x90,
            0xc6,
            0xeb,
            u8::try_from(128 - at - 12).unwrap(),
        ]);
    }
    bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
    bytes
}

#[test]
fn actual_byte_accumulator_products_capture_current_al_and_source_twice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the actual engine Wasm first");
    let expected = std::env::var("RING3_ENGINE_SHA256")
        .expect("provide the captured current engine SHA-256 before actual execution");
    assert!(
        expected.len() == 64
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    let output = root
        .join("target/p2-byte-accumulator-multiply-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 1).unwrap().arena(),
    )
    .unwrap();
    for (kind, field) in [("unsigned", 0x20u8), ("signed", 0x28)] {
        fs::write(output.join(format!("chain-{kind}.x86")), bank(field)).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-accumulator-multiply/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .arg(ARENA_SIZE.to_string())
        .env("RING3_ENGINE_SHA256", expected)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual byte accumulator integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
