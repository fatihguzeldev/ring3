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
fn logical_integer_modules_execute_against_independent_llvm_and_unsigned_oracles() {
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
    let output = root.join("target/p2-logical-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    for (name, bytes) in [
        ("and_rm_reg", &[0x21, 0xd8][..]),
        ("and_reg_rm", &[0x23, 0xc3]),
        ("and_eax_imm32", &[0x25, 3, 1, 0, 0x80]),
        ("and_modrm_imm32", &[0x81, 0xe0, 3, 1, 0, 0x80]),
        ("and_imm8_min", &[0x83, 0xe0, 0x80]),
        ("and_imm8_minus1", &[0x83, 0xe0, 0xff]),
        ("and_imm8_max", &[0x83, 0xe0, 0x7f]),
        ("or_rm_reg", &[0x09, 0xd8]),
        ("or_reg_rm", &[0x0b, 0xc3]),
        ("or_eax_imm32", &[0x0d, 3, 1, 0, 0x80]),
        ("or_modrm_imm32", &[0x81, 0xc8, 3, 1, 0, 0x80]),
        ("or_imm8_min", &[0x83, 0xc8, 0x80]),
        ("or_imm8_minus1", &[0x83, 0xc8, 0xff]),
        ("or_imm8_max", &[0x83, 0xc8, 0x7f]),
        ("xor_rm_reg", &[0x31, 0xd8]),
        ("xor_reg_rm", &[0x33, 0xc3]),
        ("xor_eax_imm32", &[0x35, 3, 1, 0, 0x80]),
        ("xor_modrm_imm32", &[0x81, 0xf0, 3, 1, 0, 0x80]),
        ("xor_imm8_min", &[0x83, 0xf0, 0x80]),
        ("xor_imm8_minus1", &[0x83, 0xf0, 0xff]),
        ("xor_imm8_max", &[0x83, 0xf0, 0x7f]),
        ("test_rm_reg", &[0x85, 0xd8]),
        ("test_eax_imm32", &[0xa9, 3, 1, 0, 0x80]),
        ("test_modrm_imm32", &[0xf7, 0xc0, 3, 1, 0, 0x80]),
        ("and_esp_same", &[0x21, 0xe4]),
        ("or_esp_eax", &[0x09, 0xc4]),
        ("xor_eax_esp", &[0x31, 0xe0]),
        ("test_esp_same", &[0x85, 0xe4]),
        ("checkpoint", &[0x21, 0xd8, 0x09, 0xc4, 0x85, 0xc0]),
    ] {
        artifact(&output, name, bytes);
    }
    for condition in 0..16 {
        artifact(
            &output,
            &format!("jcc_{condition}"),
            &[0x85, 0xc0, 0x70 + condition, 2],
        );
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-logical/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "logical Node proof failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
