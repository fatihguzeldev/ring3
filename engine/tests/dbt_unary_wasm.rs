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
fn unary_modules_execute_against_independent_llvm_and_flags_oracles() {
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
    let output = root.join("target/p2-unary-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    for (name, bytes) in [
        ("inc_short", &[0x40][..]),
        ("inc_modrm", &[0xff, 0xc0]),
        ("dec_short", &[0x48]),
        ("dec_modrm", &[0xff, 0xc8]),
        ("not_eax", &[0xf7, 0xd0]),
        ("neg_eax", &[0xf7, 0xd8]),
        ("inc_ecx", &[0x41]),
        ("dec_edx", &[0x4a]),
        ("not_ebx", &[0xf7, 0xd3]),
        ("neg_esp", &[0xf7, 0xdc]),
        ("inc_ebp_modrm", &[0xff, 0xc5]),
        ("dec_esi", &[0x4e]),
        ("not_edi", &[0xf7, 0xd7]),
        ("checkpoint", &[0x40, 0x4c, 0xf7, 0xd0, 0xf7, 0xdc]),
        ("inc_jc", &[0x40, 0x72, 2]),
        ("dec_jnc", &[0x48, 0x73, 2]),
        ("neg_jc", &[0xf7, 0xd8, 0x72, 2]),
        ("not_jc", &[0xf7, 0xd0, 0x72, 2]),
    ] {
        artifact(&output, name, bytes);
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-unary/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "unary Node proof failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
