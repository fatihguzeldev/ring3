use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ring3_engine::cpu::dbt::CompileLimits;

fn write_bank(output: &Path, name: &str, forms: &[Vec<u8>], canary: bool, packed: usize) {
    let mut bytes = [0xcc; 130];
    let mut offset = 0;
    for form in forms {
        let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
        if canary {
            tail.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
        }
        let length = form.len() + tail.len() + 2;
        bytes[offset..offset + form.len()].copy_from_slice(form);
        bytes[offset + form.len()..offset + length - 2].copy_from_slice(&tail);
        bytes[offset + length - 2..offset + length]
            .copy_from_slice(&[0xeb, u8::try_from(128 - offset - length).unwrap()]);
        offset += length;
    }
    assert_eq!(offset, packed);
    assert!(forms.len() <= 8);
    bytes[128..].copy_from_slice(&[0x0f, 0x0b]);
    fs::write(output.join(format!("{name}.x86")), bytes).unwrap();
}

#[test]
fn actual_wasm_double_shifts_preserve_old_operands_flags_and_current_tail_continuation() {
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
    let output = root.join("target/p2-double-shift32-fixtures").join(unique);
    fs::create_dir_all(&output).unwrap();
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65_536)
    );
    for (kind, immediate, cl) in [("shld", 0xa4, 0xa5), ("shrd", 0xac, 0xad)] {
        for (mode, opcode) in [("imm", immediate), ("cl", cl)] {
            for source in 0u8..8 {
                let forms: Vec<Vec<u8>> = (0u8..8)
                    .map(|destination| {
                        let mut bytes = vec![0x0f, opcode, 0xc0 | source << 3 | destination];
                        if mode == "imm" {
                            bytes.push(1);
                        }
                        bytes
                    })
                    .collect();
                write_bank(
                    &output,
                    &format!("normal-{kind}-{mode}-s{source}"),
                    &forms,
                    false,
                    if mode == "imm" { 96 } else { 88 },
                );
            }
        }
        let forms: Vec<Vec<u8>> = [0, 32, 64, 128, 2, 31, 33, 255]
            .into_iter()
            .map(|raw| vec![0x0f, immediate, 0xfe, raw])
            .collect();
        write_bank(&output, &format!("edge-{kind}-imm"), &forms, false, 96);
    }
    let cl_edges = vec![
        vec![0x0f, 0xa5, 0xcb],
        vec![0x0f, 0xa5, 0xf9],
        vec![0x0f, 0xad, 0xcb],
        vec![0x0f, 0xad, 0xf9],
    ];
    write_bank(&output, "edge-cl", &cl_edges, false, 44);
    let currency = vec![vec![0xbe, 0x78, 0x56, 0x34, 0x12, 0x0f, 0xac, 0xfe, 1]];
    write_bank(&output, "currency", &currency, true, 22);
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-double-shift32/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual register double-shift integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
