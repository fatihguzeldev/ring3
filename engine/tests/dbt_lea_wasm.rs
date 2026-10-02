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
fn lea_modules_execute_against_independent_llvm_and_modulo_address_oracles() {
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
    let output = root.join("target/p2-lea-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    for (name, bytes) in [
        ("lea_eax_base", &[0x8d, 0x03][..]),
        (
            "lea_ecx_index2",
            &[0x8d, 0x0c, 0x55, 0x78, 0x56, 0x34, 0x12],
        ),
        ("lea_edx_scale4", &[0x8d, 0x54, 0xb3, 0x80]),
        (
            "lea_ebx_scale8",
            &[0x8d, 0x9c, 0xf9, 0x78, 0x56, 0x34, 0x12],
        ),
        ("lea_esp_baseesp", &[0x8d, 0x64, 0x04, 0xff]),
        ("lea_ebp_index_alias", &[0x8d, 0x6c, 0x6a, 0x7f]),
        ("lea_esi_base_alias", &[0x8d, 0xb4, 0x8e, 0, 0, 0, 0x80]),
        ("lea_edi_both_alias", &[0x8d, 0x7c, 0xff, 7]),
        (
            "lea_no_base_index1",
            &[0x8d, 0x04, 0x35, 0x21, 0x43, 0x65, 0x87],
        ),
        ("lea_absolute", &[0x8d, 0x05, 0xff, 0xff, 0xff, 0xff]),
        (
            "lea_index8_only",
            &[0x8d, 0x04, 0xdd, 0xff, 0xff, 0xff, 0xff],
        ),
        ("lea_ebp_no_index", &[0x8d, 0x45, 0]),
        ("lea_esp_no_index", &[0x8d, 0x04, 0x24]),
        ("lea_disp32_positive", &[0x8d, 0x83, 0xff, 0xff, 0xff, 0x7f]),
        ("lea_disp32_negative", &[0x8d, 0x83, 0, 0, 0, 0x80]),
        (
            "checkpoint",
            &[0x8d, 0x44, 0x8b, 0xff, 0x8d, 0x24, 0x04, 0x8d, 0x04, 0x40],
        ),
    ] {
        artifact(&output, name, bytes);
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-lea/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "LEA Node proof failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
