use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::{BlockSpec, CompileLimits, compile_region};
use ring3_engine::memory::{AddressSpace, GuestAddress, PageRange, Permissions};

fn artifact(output: &Path, name: &str, bytes: &[u8]) {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: bytes.len() as u32,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    fs::write(
        output.join(format!("{name}.wasm")),
        compiled.wasm_bytes(&memory).unwrap(),
    )
    .unwrap();
}

#[test]
fn register_extensions_execute_against_independent_llvm_and_extraction_oracles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine Wasm cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-extensions-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    // authored ModRM register forms are checked independently against LLVM.
    let byte_sources = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
    let word_sources = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
    let byte_destinations = [0, 1, 2, 3, 0, 5, 6, 7];
    for (kind, byte_opcode, word_opcode) in [("zx", 0xb6, 0xb7), ("sx", 0xbe, 0xbf)] {
        for source in 0..8 {
            artifact(
                &output,
                &format!("{kind}_byte_{}", byte_sources[source]),
                &[
                    0x0f,
                    byte_opcode,
                    0xc0 + byte_destinations[source] * 8 + source as u8,
                ],
            );
            artifact(
                &output,
                &format!("{kind}_word_{}", word_sources[source]),
                &[0x0f, word_opcode, 0xc0 + source as u8 * 9],
            );
        }
    }
    for (name, bytes) in [
        (
            "checkpoint",
            &[
                0x0f, 0xbe, 0xc4, 0x0f, 0xb7, 0xe4, 0x0f, 0xbf, 0xe4, 0x0f, 0xb6, 0xc0,
            ][..],
        ),
        ("sx_ah_jc", &[0x0f, 0xbe, 0xc4, 0x72, 2]),
        ("zx_sp_jno", &[0x0f, 0xb7, 0xe4, 0x71, 2]),
        ("sx_di_jz", &[0x0f, 0xbf, 0xff, 0x74, 2]),
        ("zx_cl_js", &[0x0f, 0xb6, 0xc9, 0x78, 2]),
    ] {
        artifact(&output, name, bytes);
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-extensions/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "register-extension Node proof failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
