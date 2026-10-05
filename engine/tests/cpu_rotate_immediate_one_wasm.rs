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

#[test]
fn actual_wasm_accepts_only_masked_one_rotate_immediates() {
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
    let output = root
        .join("target/p2-rotate-immediate-one-fixtures")
        .join(unique);
    fs::create_dir_all(&output).unwrap();
    let counts = [1, 33, 65, 97, 129, 161, 193, 225];
    for (kind, extension) in [("left", 0), ("right", 8)] {
        for bank in 0..2 {
            let mut bytes = vec![0xcc; if bank == 0 { 172 } else { 162 }];
            let mut specs = Vec::new();
            let mut offset = 0;
            for slot in 0..4 {
                let entry = offset;
                if slot == 0 {
                    bytes[offset..offset + 3].copy_from_slice(&[0x83, 0xc3, 1]);
                    offset += 3;
                }
                for count in counts {
                    bytes[offset..offset + 3].copy_from_slice(&[
                        0xc1,
                        0xc0 | extension | u8::try_from(bank * 4 + slot).unwrap(),
                        count,
                    ]);
                    offset += 3;
                }
                bytes[offset..offset + 8].copy_from_slice(&[
                    0x0f,
                    0x92,
                    0xc0,
                    0x0f,
                    0x90,
                    0xc2,
                    0xeb,
                    u8::try_from(160 - offset - 8).unwrap(),
                ]);
                offset += 8;
                specs.push(BlockSpec {
                    entry: GuestAddress(0x1000 + u32::try_from(entry).unwrap()),
                    byte_length: u32::try_from(offset - entry).unwrap(),
                });
            }
            assert_eq!(offset, 131);
            bytes[160..162].copy_from_slice(&[0x0f, 0x0b]);
            if bank == 0 {
                bytes[162..].copy_from_slice(&[
                    0xd1,
                    0xc0 | extension,
                    0x0f,
                    0x92,
                    0xc0,
                    0x0f,
                    0x90,
                    0xc2,
                    0xeb,
                    0xf4,
                ]);
            }
            let mut memory = AddressSpace::new(1).unwrap();
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
            let compiled = compile_region(&memory, &specs, CompileLimits::default()).unwrap();
            assert_eq!(compiled.metadata().blocks, 4);
            assert_eq!(compiled.metadata().instructions, 45);
            let name = format!("register-{kind}-{bank}");
            fs::write(output.join(format!("{name}.x86")), &bytes).unwrap();
            fs::write(
                output.join(format!("standalone-{name}.wasm")),
                compiled.wasm_bytes(&memory).unwrap(),
            )
            .unwrap();
            if bank == 0 {
                let control = compile_region(
                    &memory,
                    &[BlockSpec {
                        entry: GuestAddress(0x10a2),
                        byte_length: 10,
                    }],
                    CompileLimits::default(),
                )
                .unwrap();
                assert_eq!(control.metadata().instructions, 4);
                fs::write(
                    output.join(format!("standalone-d1-{kind}.wasm")),
                    control.wasm_bytes(&memory).unwrap(),
                )
                .unwrap();
            }
        }
        let mut bytes = vec![0xcc; 144];
        let forms = [
            vec![
                0x83,
                0xc3,
                1,
                0xc1,
                5 | extension,
                0x10,
                0x40,
                0,
                0,
                counts[0],
            ],
            vec![0xc1, 0x40 | extension, 0x10, counts[1]],
            vec![0xc1, 0x42 | extension, 0xfe, counts[2]],
            vec![0xc1, 0x44 | extension, 0x24, 8, counts[3]],
            vec![0xc1, 0x44 | extension, 0x87, 0x10, counts[4]],
            vec![0xc1, 0x44 | extension, 0x57, 0xfe, counts[5]],
            vec![0x8d, 0x5b, 1, 0xc1, 7 | extension, counts[6]],
            vec![0xc1, 7 | extension, counts[7]],
        ];
        let mut offset = 0;
        for (index, form) in forms.iter().enumerate() {
            let mut tail = vec![0x0f, 0x92, 0xc0, 0x0f, 0x90, 0xc2];
            if index == 7 {
                tail.extend_from_slice(&[0xbb, 0xef, 0xbe, 0xad, 0xde]);
            }
            let length = form.len() + tail.len() + 2;
            bytes[offset..offset + form.len()].copy_from_slice(form);
            bytes[offset + form.len()..offset + length - 2].copy_from_slice(&tail);
            bytes[offset + length - 2..offset + length]
                .copy_from_slice(&[0xeb, u8::try_from(128 - offset - length).unwrap()]);
            offset += length;
        }
        assert_eq!(offset, 111);
        bytes[128..130].copy_from_slice(&[0x0f, 0x0b]);
        bytes[130..].copy_from_slice(&[
            0xd1,
            5 | extension,
            0x10,
            0x40,
            0,
            0,
            0x0f,
            0x92,
            0xc0,
            0x0f,
            0x90,
            0xc2,
            0xeb,
            0xf0,
        ]);
        fs::write(output.join(format!("memory-{kind}.x86")), bytes).unwrap();
    }
    let result = Command::new("node")
        .arg(root.join("engine/tests/fixtures/p2-rotate-immediate-one/run.mjs"))
        .arg(&engine)
        .arg(&output)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "actual masked-one rotate integration failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout).trim());
}
