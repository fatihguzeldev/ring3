use super::support_wasm as wasm_support;
use ring3_engine::{
    cpu::x86::{decode::decode_one, ir::Operation, simd::SimdKind},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_upper_state_transport_preserves_legacy_effects_and_callback_identity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = std::env::var_os("RING3_ENGINE_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm"));
    assert!(engine.is_file());
    let output = root.join("target/p2-ymm-state-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("initial-arena.bin"),
        EngineInstance::new(1, 91).unwrap().arena(),
    )
    .unwrap();
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut descriptors = Vec::new();
    for (code, kind) in [
        ([0x0f, 0xae, 0x07], SimdKind::SaveExtended),
        ([0x0f, 0xae, 0x0f], SimdKind::RestoreExtended),
    ] {
        memory.write(GuestAddress(0x1000), &code).unwrap();
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let Operation::Simd(operation) = decoded.operation() else {
            panic!("legacy extended state instruction")
        };
        assert_eq!(operation.kind, kind);
        let (descriptor, operands, immediate) = operation.descriptor().unwrap();
        for value in [descriptor, operands, immediate] {
            descriptors.extend_from_slice(&value.to_le_bytes());
        }
    }
    fs::write(output.join("fx-descriptors.bin"), descriptors).unwrap();
    let result = wasm_support::run_node(
        root,
        "engine/tests/cpu/simd/state/upper/fixtures/p2-ymm-state/runtime/run.mjs",
        &engine,
        &output,
        None,
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "upper state actual failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
