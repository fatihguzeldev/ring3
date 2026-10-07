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

fn save(
    output: &Path,
    name: &str,
    bytes: &[u8; 130],
    spans: &[(usize, usize)],
    instructions: usize,
    pure: bool,
) {
    fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
    if pure {
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
        let compiled = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (spans.len(), instructions)
        );
        fs::write(
            output.join(format!("standalone-{name}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
}

fn tail(bytes: &mut [u8; 130], at: usize, canary: bool) -> usize {
    let mut code = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
    if canary {
        code.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
    }
    code.extend_from_slice(&[0xeb, u8::try_from(128 - at - code.len() - 2).unwrap()]);
    bytes[at..at + code.len()].copy_from_slice(&code);
    code.len()
}

#[test]
fn native_pure_and_actual_bound_byte_cl_rotates_capture_each_old_count() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let output = root.join("target/p2-byte-rotate-cl-fixtures").join(format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for (kind, field) in [("left", 0u8), ("right", 8)] {
        let mut bytes = [0xcc; 130];
        let mut spans = Vec::new();
        let mut at = 0;
        for alias in 0u8..8 {
            bytes[at..at + 2].copy_from_slice(&[0xd2, 0xc0 | field | alias]);
            let length = 2 + tail(&mut bytes, at + 2, false);
            spans.push((at, length));
            at += length;
        }
        assert_eq!(at, 80);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        save(&output, &format!("basis-{kind}"), &bytes, &spans, 32, true);
        let mut bytes = [0xcc; 130];
        let mut spans = Vec::new();
        let mut at = 0;
        for alias in [0u8, 1, 5] {
            let operand = 0xc0 | field | alias;
            bytes[at..at + 4].copy_from_slice(&[0xd2, operand, 0xd2, operand]);
            let length = 4 + tail(&mut bytes, at + 4, true);
            spans.push((at, length));
            at += length;
        }
        assert_eq!(at, 51);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        save(&output, &format!("chain-{kind}"), &bytes, &spans, 18, true);
    }
    for (name, operand) in [
        ("al-same", 0xc0),
        ("cl-direction", 0xc1),
        ("ch-direction", 0xcd),
    ] {
        let mut bytes = [0xcc; 130];
        bytes[..2].copy_from_slice(&[0xd2, operand]);
        assert_eq!(2 + tail(&mut bytes, 2, true), 15);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        save(
            &output,
            &format!("currency-{name}"),
            &bytes,
            &[(0, 15)],
            5,
            false,
        );
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-rotate-cl/run.mjs"))
        .arg(engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "byte CL pure/bound integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
