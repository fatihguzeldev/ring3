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

const RAW: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 24, 31, 32, 33, 255];

fn pure(output: &Path, name: &str, bytes: &[u8], spans: &[(usize, usize)], instructions: usize) {
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

fn local_tail(bytes: &mut [u8], at: usize) -> usize {
    bytes[at..at + 11].copy_from_slice(&[
        0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2, 0xbb, 0xef, 0xbe, 0xad, 0xde,
    ]);
    bytes[at + 11..at + 13].copy_from_slice(&[0xeb, u8::try_from(128 - at - 13).unwrap()]);
    13
}

#[test]
fn native_pure_and_actual_bound_immediate_byte_rotates_keep_instruction_count_authority() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let output = root
        .join("target/p2-byte-rotate-immediate-fixtures")
        .join(format!(
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
        let mut bytes = [0xcc; 4096];
        for (group, raw) in RAW.iter().copied().enumerate() {
            let mut spans = Vec::new();
            for alias in 0u8..8 {
                let at = group * 112 + usize::from(alias) * 14;
                bytes[at..at + 9].copy_from_slice(&[
                    0xc0,
                    0xc0 | field | alias,
                    raw,
                    0x0f,
                    0x92,
                    0xc0,
                    0x0f,
                    0x90,
                    0xc2,
                ]);
                bytes[at + 9] = 0xe9;
                let displacement = 0xf00i32 - i32::try_from(at + 14).unwrap();
                bytes[at + 10..at + 14].copy_from_slice(&displacement.to_le_bytes());
                spans.push((at, 14));
            }
            bytes[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
            pure(
                &output,
                &format!("master-{kind}-g{group:02}"),
                &bytes,
                &spans,
                32,
            );
        }
        fs::write(output.join(format!("master-{kind}.x86")), bytes).unwrap();
    }
    let mut bytes = [0xcc; 130];
    bytes[..6].copy_from_slice(&[0xc0, 0xc1, 1, 0xc0, 0xc1, 2]);
    assert_eq!(6 + local_tail(&mut bytes, 6), 19);
    bytes[19..25].copy_from_slice(&[0xc0, 0xcd, 9, 0xc0, 0xcd, 2]);
    assert_eq!(6 + local_tail(&mut bytes, 25), 19);
    bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
    fs::write(output.join("chain.x86"), bytes).unwrap();
    pure(&output, "chain", &bytes, &[(0, 19), (19, 19)], 12);
    for (name, operand, raw) in [
        ("al-same", 0xc0u8, 9u8),
        ("cl-immediate", 0xc9, 9),
        ("ch-direction", 0xc5, 9),
    ] {
        let mut bytes = [0xcc; 130];
        bytes[..3].copy_from_slice(&[0xc0, operand, raw]);
        assert_eq!(3 + local_tail(&mut bytes, 3), 16);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        fs::write(output.join(format!("currency-{name}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-byte-rotate-immediate/run.mjs"))
        .arg(engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "immediate byte pure/bound integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
