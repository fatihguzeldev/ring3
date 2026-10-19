use super::support_wasm;
use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn actual_resident_x87_pairs_preserve_scalar_boundaries_and_receipt_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = support_wasm::selected_engine(root);
    let expected = std::env::var("RING3_ENGINE_SHA256")
        .expect("provide the selected engine SHA-256 with RING3_ENGINE_SHA256");
    let output = root.join("target/p2-x87-pairs").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let code = [0xdb, 0xe3, 0xdb, 0xe2];
    let mut source = EngineInstance::new(1, 7).unwrap();
    source.map(0x1000, 1, 7).unwrap();
    source.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    source.upload(0x1000, code.len() as u32).unwrap();
    let unbound = compile_region(
        source.memory().unwrap(),
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: code.len() as u32,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    fs::write(
        output.join("unbound-control.wasm"),
        unbound.wasm_bytes(source.memory().unwrap()).unwrap(),
    )
    .unwrap();
    let result = support_wasm::run_node(
        root,
        "engine/tests/cpu/x87/pairs/fixtures/p2-x87-pairs/runtime/run.mjs",
        &engine,
        &output,
        Some(&expected),
    );
    fs::write(output.join("node.stdout.log"), &result.stdout).unwrap();
    fs::write(output.join("node.stderr.log"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
