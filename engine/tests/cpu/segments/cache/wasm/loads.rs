use super::support_wasm as wasm_support;

use ring3_engine::{
    abi::{
        arena::X87_OFFSET,
        x86::{X87_SIZE, decode_x87},
    },
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_segment_reads_descriptor_loads_and_plain_strings_preserve_precise_state() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    let output = root.join("target/p2-segment-loads-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let mut initial = EngineInstance::new(1, 7).unwrap().arena().to_vec();
    initial[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    initial[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in initial[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    decode_x87(&initial[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    fs::write(output.join("initial-arena.bin"), initial).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/segments/cache/fixtures/p2-segment-loads/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "segment integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
