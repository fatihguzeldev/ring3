use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::arena::{ARENA_SIZE, X87_OFFSET},
    process::EngineInstance,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::support_compiled_switch as pe;

#[test]
fn actual_compiler_switch_uses_relocated_readonly_table_and_live_caller() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file(), "build the integrated engine wasm first");
    let output = root
        .join("target/p2-compiled-switch-fixtures")
        .join(format!(
            "wasm-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    assert_eq!(ARENA_SIZE, 4612);
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let fixture = pe::fixture();
    fs::write(output.join("switch.exe"), &fixture.image).unwrap();
    fs::write(output.join("manifest.json"), fixture.manifest_json()).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/programs/dispatch/fixtures/p2-compiled-switch/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node-stdout.txt"), &result.stdout).unwrap();
    fs::write(output.join("node-stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "compiled switch integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
