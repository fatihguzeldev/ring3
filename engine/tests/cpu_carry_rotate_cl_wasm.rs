use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn pure(output: &Path, name: &str, bytes: &[u8], spans: &[(usize, usize)], instructions: usize) {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let specs: Vec<_> = spans
        .iter()
        .map(|&(at, length)| BlockSpec {
            entry: GuestAddress(0x1000 + u32::try_from(at).unwrap()),
            byte_length: u32::try_from(length).unwrap(),
        })
        .collect();
    let compiled = compile_region(&memory, &specs, limits).unwrap();
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (spans.len(), instructions)
    );
    let module = compiled.wasm_bytes(&memory).unwrap();
    assert!(module.len() <= limits.wasm_bytes);
    fs::write(output.join(format!("standalone-{name}.wasm")), module).unwrap();
}

fn local_tail(bytes: &mut [u8], at: usize) -> usize {
    let code = [
        0x0f,
        0x92,
        0xc0,
        0x0f,
        0x90,
        0xc2,
        0xbb,
        0xef,
        0xbe,
        0xad,
        0xde,
        0xeb,
        u8::try_from(128 - at - 13).unwrap(),
    ];
    bytes[at..at + code.len()].copy_from_slice(&code);
    code.len()
}

#[test]
fn native_pure_and_actual_bound_dword_carry_rotates_by_cl() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the current actual engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root
        .join("target/p2-carry-rotate-cl32-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let mut bytes = vec![0xcc; 4096];
    let mut at = 0usize;
    let mut groups = Vec::new();
    for field in [0x10u8, 0x18] {
        let mut spans = Vec::new();
        for alias in 0u8..8 {
            let mut code = vec![
                0xd3,
                0xc0 | field | alias,
                0x0f,
                0x92,
                0xc0,
                0x0f,
                0x90,
                0xc2,
                0xe9,
            ];
            code.extend_from_slice(&i32::try_from(0xf00 - at - 13).unwrap().to_le_bytes());
            assert_eq!(code.len(), 13);
            bytes[at..at + code.len()].copy_from_slice(&code);
            spans.push((at, code.len()));
            at += code.len();
        }
        assert_eq!(spans.len(), 8);
        groups.push(spans);
    }
    assert_eq!(at, 208);
    bytes[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    fs::write(output.join("master.x86"), &bytes).unwrap();
    for (group, spans) in groups.iter().enumerate() {
        pure(&output, &format!("master-g{group}"), &bytes, spans, 32);
    }
    let mut bytes = [0xcc; 130];
    let mut spans = Vec::new();
    let mut at = 0;
    for field in [0x10u8, 0x18] {
        for (producer, destination) in [(0xf8u8, 1u8), (0xf9, 4), (0xf5, 5)] {
            let operand = 0xc0 | field | destination;
            bytes[at..at + 5].copy_from_slice(&[producer, 0xd3, operand, 0xd3, operand]);
            let length = 5 + local_tail(&mut bytes, at + 5);
            spans.push((at, length));
            at += length;
        }
    }
    assert_eq!(at, 108);
    bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
    fs::write(output.join("chain.x86"), bytes).unwrap();
    pure(&output, "chain", &bytes, &spans, 42);
    for (name, operand) in [
        ("eax-same-opcode", 0xd0u8),
        ("ecx-direction", 0xd1),
        ("esp-destination", 0xdc),
    ] {
        let mut bytes = [0xcc; 130];
        bytes[..2].copy_from_slice(&[0xd3, operand]);
        assert_eq!(2 + local_tail(&mut bytes, 2), 15);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        fs::write(output.join(format!("currency-{name}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-carry-rotate-cl32/run.mjs"))
        .arg(engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "dword carry cl pure/bound integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
