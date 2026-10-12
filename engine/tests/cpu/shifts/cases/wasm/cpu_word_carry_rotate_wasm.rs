use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const CODE: u32 = 0x1000;

fn hex(bytes: &str) -> Vec<u8> {
    bytes
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn capture_word_carry_standalone_profiles_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_CARRY_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    }
    let mut bank = vec![0xcc; 4096];
    let mut at = 0;
    let mut start = 0;
    let mut count = 0;
    let mut main = Vec::new();
    let high = [32, 33, 49, 50, 255, 0x66, 0x67, 0xf0, 0xf2, 0xf3];
    for opcode in [0xd1, 0xc1, 0xd3] {
        let mut forms = Vec::new();
        if opcode == 0xc1 {
            for kind in 0..2 {
                for raw in 0..32 {
                    forms.push((kind, 0, Some(raw)));
                }
            }
            for kind in 0..2 {
                for raw in high {
                    forms.push((kind, 0, Some(raw)));
                }
            }
            for kind in 0..2 {
                for alias in 1..8 {
                    forms.push((kind, alias, Some(18)));
                }
            }
        } else {
            for kind in 0..2 {
                for alias in 0..8 {
                    forms.push((kind, alias, None));
                }
            }
        }
        for (kind, alias, raw) in forms {
            bank[at..at + 3].copy_from_slice(&[0x66, opcode, 0xd0 + kind * 8 + alias]);
            at += 3;
            if let Some(raw) = raw {
                bank[at] = raw;
                at += 1;
            }
            count += 1;
            if [48, 96, 130].contains(&count) {
                main.push(BlockSpec {
                    entry: GuestAddress(CODE + start as u32),
                    byte_length: (at - start) as u32,
                });
                bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
                at += 2;
                start = at;
            }
        }
    }
    assert_eq!(count, 130);
    let mut consumer = Vec::new();
    at = 2048;
    for (index, bytes) in [
        "66b9218066d3d166d3d966d3d10f92c20f90c366b8008066d1d00f92c60f90c76683d50066b8008066c1d8120f90c07102",
        "66b8008066d1d07002",
        "0f90c2d0d0d1deebfe",
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = hex(bytes);
        consumer.push(BlockSpec {
            entry: GuestAddress(CODE + at as u32),
            byte_length: bytes.len() as u32,
        });
        bank[at..at + bytes.len()].copy_from_slice(&bytes);
        at += bytes.len();
        if index < 2 {
            bank[at..at + 2].copy_from_slice(&[0x0f, 0x0b]);
            at += 2;
        }
    }
    at = 2560;
    for opcode in [0xd1, 0xc1, 0xd3] {
        for kind in 0..2 {
            for alias in 0..8 {
                bank[at..at + 3].copy_from_slice(&[0x66, opcode, 0xd0 + kind * 8 + alias]);
                at += 3;
                if opcode == 0xc1 {
                    bank[at] = 18;
                    at += 1;
                }
            }
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    let entry_forms = BlockSpec {
        entry: GuestAddress(CODE + 2560),
        byte_length: (at + 2 - 2560) as u32,
    };
    bank[3840..3842].copy_from_slice(&[0x0f, 0x0b]);

    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(CODE), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(CODE), &bank).unwrap();
    let groups = [
        ("main-0", false, vec![main[0]], 48),
        ("main-1", false, vec![main[1]], 48),
        ("main-2", false, vec![main[2]], 34),
        ("consumer", false, consumer.clone(), 22),
        ("entry-forms", true, vec![entry_forms], 49),
        ("entry-consumer", true, consumer, 22),
    ];
    let mut captures = Vec::new();
    for (name, entries, blocks, instructions) in groups {
        let artifact = if entries {
            compile_entry_region(
                &memory,
                &blocks.iter().map(|block| block.entry).collect::<Vec<_>>(),
                CompileLimits::default(),
            )
        } else {
            compile_region(&memory, &blocks, CompileLimits::default())
        }
        .unwrap();
        assert_eq!(artifact.metadata().blocks, blocks.len());
        assert_eq!(artifact.metadata().instructions, instructions);
        let bytes = artifact.wasm_bytes(&memory).unwrap();
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
        captures.push((format!("{name}.wasm"), bytes.to_vec()));
    }
    captures.push(("bank.x86".into(), bank));
    captures.push((
        "initial-arena.bin".into(),
        EngineInstance::new(1, 1).unwrap().arena().to_vec(),
    ));
    if let Some(directory) = directory {
        let mut manifest = Vec::new();
        for (name, bytes) in captures {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(&name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            manifest.push(format!("{{\"file\":\"{name}\",\"bytes\":{}}}", bytes.len()));
        }
        let json = format!(
            "{{\"schema_version\":1,\"files\":[{}]}}\n",
            manifest.join(",")
        );
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("capture.json"))
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
}
