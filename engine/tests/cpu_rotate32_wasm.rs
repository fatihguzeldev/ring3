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

fn save_bank(
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
        let specs: Vec<BlockSpec> = spans
            .iter()
            .map(|&(offset, length)| BlockSpec {
                entry: GuestAddress(0x1000 + u32::try_from(offset).unwrap()),
                byte_length: u32::try_from(length).unwrap(),
            })
            .collect();
        let compiled = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
        assert_eq!(compiled.metadata().blocks, spans.len());
        assert_eq!(compiled.metadata().instructions, instructions);
        fs::write(
            output.join(format!("standalone-{name}.wasm")),
            compiled.wasm_bytes(&memory).unwrap(),
        )
        .unwrap();
    }
}

fn tail(bytes: &mut [u8; 130], offset: usize, canary: bool) -> usize {
    let mut code = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
    if canary {
        code.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
    }
    code.extend_from_slice(&[0xeb, u8::try_from(128 - offset - code.len() - 2).unwrap()]);
    bytes[offset..offset + code.len()].copy_from_slice(&code);
    code.len()
}

#[test]
fn actual_wasm_register_rotates_preserve_old_count_flags_and_current_tail() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let engine = root.join("target/wasm32-unknown-unknown/debug/ring3_engine.wasm");
    assert!(
        engine.is_file(),
        "build the actual engine wasm32 cdylib first"
    );
    let unique = format!(
        "wasm-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let output = root.join("target/p2-rotate32-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for (kind, extension) in [("left", 0u8), ("right", 8)] {
        for destination in 0u8..8 {
            let mut bytes = [0xcc; 130];
            for count in 0u8..32 {
                let offset = usize::from(count) * 3;
                bytes[offset..offset + 3].copy_from_slice(&[
                    0xc1,
                    0xc0 | extension | destination,
                    count,
                ]);
            }
            bytes[96..98].copy_from_slice(&[0xd3, 0xc0 | extension | destination]);
            assert_eq!(98 + tail(&mut bytes, 98, false), 106);
            bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
            save_bank(
                &output,
                &format!("base-{kind}-d{destination}"),
                &bytes,
                &[(0, 106)],
                36,
                true,
            );
        }
        let mut bytes = [0xcc; 130];
        let mut spans = Vec::new();
        let mut offset = 0;
        for count in [0, 32, 64, 128, 33, 65, 129, 255] {
            bytes[offset..offset + 3].copy_from_slice(&[0xc1, 0xc0 | extension, count]);
            let length = 3 + tail(&mut bytes, offset + 3, false);
            spans.push((offset, length));
            offset += length;
        }
        assert_eq!(offset, 88);
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        save_bank(
            &output,
            &format!("edge-{kind}-imm"),
            &bytes,
            &spans,
            32,
            true,
        );
    }
    for case in ["opcode", "direction", "immediate"] {
        let mut bytes = [0xcc; 130];
        bytes[..5].copy_from_slice(&[0xb8, 0x78, 0x56, 0x34, 0x12]);
        let target: &[u8] = if case == "immediate" {
            &[0xc1, 0xc6, 2]
        } else {
            &[0xd3, 0xc6]
        };
        bytes[5..5 + target.len()].copy_from_slice(target);
        let packed = 5 + target.len() + tail(&mut bytes, 5 + target.len(), true);
        assert_eq!(packed, if case == "immediate" { 21 } else { 20 });
        bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
        save_bank(
            &output,
            &format!("currency-{case}"),
            &bytes,
            &[(0, packed)],
            6,
            false,
        );
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-rotate32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual register rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
