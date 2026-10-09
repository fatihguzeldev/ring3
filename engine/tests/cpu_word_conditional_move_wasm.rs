use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

fn legacy_banks() -> [(&'static str, Vec<u8>); 2] {
    let mut conditional = Vec::new();
    for alias in [false, true] {
        for condition in 0_u8..16 {
            let destination = condition % 8;
            let source = if alias {
                destination
            } else {
                (destination + 3) % 8
            };
            conditional.extend([0x0f, 0x40 + condition, 0xc0 | destination << 3 | source]);
        }
    }
    conditional.extend([0xeb, 0xfe]);
    let mut moves = Vec::new();
    for opcode in [0x89, 0x8b] {
        for alias in [false, true] {
            for destination in 0_u8..8 {
                let source = if alias {
                    destination
                } else {
                    (destination + 3) % 8
                };
                let modrm = if opcode == 0x89 {
                    source << 3 | destination
                } else {
                    destination << 3 | source
                };
                moves.extend([0x66, opcode, 0xc0 | modrm]);
            }
        }
    }
    moves.extend([0xeb, 0xfe]);
    [
        ("legacy-dword-cmov", conditional),
        ("legacy-word-register-mov", moves),
    ]
}

#[test]
fn capture_legacy_conditional_and_word_moves_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_CMOV_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let mut memory = AddressSpace::new(2).unwrap();
    let mut captures = Vec::new();
    for (index, (name, bytes)) in legacy_banks().into_iter().enumerate() {
        assert_eq!(bytes.len(), 98);
        let pc = GuestAddress(0x1000 + index as u32 * 0x1000);
        memory
            .map_zeroed(PageRange::new(pc, 1).unwrap(), Permissions::ALL)
            .unwrap();
        memory.write(pc, &bytes).unwrap();
        for entries in [false, true] {
            let artifact = if entries {
                compile_entry_region(&memory, &[pc], CompileLimits::default())
            } else {
                compile_region(
                    &memory,
                    &[BlockSpec {
                        entry: pc,
                        byte_length: bytes.len() as u32,
                    }],
                    CompileLimits::default(),
                )
            }
            .unwrap();
            assert_eq!(artifact.metadata().instructions, 33);
            assert_eq!(artifact.metadata().blocks, 1);
            let wasm = artifact.wasm_bytes(&memory).unwrap();
            assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
            let profile = if entries { "entry" } else { "explicit" };
            captures.push((format!("{name}-{profile}.wasm"), wasm.to_vec()));
        }
        captures.push((format!("{name}.x86"), bytes));
    }
    assert_eq!(captures.len(), 6);
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
        let manifest = format!(
            "{{\"schema_version\":1,\"legacy_only\":true,\"files\":[{}]}}\n",
            manifest.join(",")
        );
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("legacy-capture.json"))
            .unwrap()
            .write_all(manifest.as_bytes())
            .unwrap();
    }
}

struct CaptureGroup {
    name: &'static str,
    entries: bool,
    blocks: Vec<BlockSpec>,
    instructions: usize,
}

fn word_conditional_bank() -> (Vec<u8>, [CaptureGroup; 5]) {
    let mut bank = vec![0xcc; 4096];
    let mut forms: Vec<(u8, u8, u8)> = (0..16).map(|condition| (condition, 0, 3)).collect();
    for destination in 0..8 {
        for source in 0..8 {
            if destination != 0 || source != 3 {
                forms.push((4, destination, source));
            }
        }
    }
    assert_eq!(forms.len(), 79);
    let (mut at, mut start) = (0, 0);
    let mut main = Vec::new();
    for (index, (condition, destination, source)) in forms.into_iter().enumerate() {
        bank[at..at + 4].copy_from_slice(&[
            0x66,
            0x0f,
            0x40 + condition,
            0xc0 | destination << 3 | source,
        ]);
        at += 4;
        if index == 47 || index == 78 {
            main.push(BlockSpec {
                entry: GuestAddress(0x1000 + start as u32),
                byte_length: (at - start) as u32,
            });
            bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
            at += 2;
            start = at;
        }
    }
    let mut consumer = Vec::new();
    for (path, raw) in [
        "6639c8660f4cd3660f4dee0f4cfe0f90c40f92c17c02",
        "6639c8660f42d3660f43ee0f42fe0f92c40f90c17202",
    ]
    .into_iter()
    .enumerate()
    {
        let bytes: Vec<u8> = (0..raw.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&raw[index..index + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 22);
        let offset = 2048 + path * 64;
        bank[offset..offset + 22].copy_from_slice(&bytes);
        bank[offset + 22..offset + 26].copy_from_slice(&[0x0f, 0x0b, 0xeb, 0]);
        consumer.extend([
            BlockSpec {
                entry: GuestAddress(0x1000 + offset as u32),
                byte_length: 22,
            },
            BlockSpec {
                entry: GuestAddress(0x1000 + offset as u32 + 24),
                byte_length: 2,
            },
        ]);
    }
    at = 2560;
    for condition in 0_u8..16 {
        bank[at..at + 4].copy_from_slice(&[
            0x66,
            0x0f,
            0x40 + condition,
            0xc0 | (condition % 8) << 3 | ((condition + 3) % 8),
        ]);
        at += 4;
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    bank[3840..3842].copy_from_slice(&[0x0f, 0x0b]);
    let groups = [
        CaptureGroup {
            name: "main-0",
            entries: false,
            blocks: vec![main[0]],
            instructions: 48,
        },
        CaptureGroup {
            name: "main-1",
            entries: false,
            blocks: vec![main[1]],
            instructions: 31,
        },
        CaptureGroup {
            name: "consumer",
            entries: false,
            blocks: consumer.clone(),
            instructions: 16,
        },
        CaptureGroup {
            name: "entry-forms",
            entries: true,
            blocks: vec![BlockSpec {
                entry: GuestAddress(0x1a00),
                byte_length: 66,
            }],
            instructions: 17,
        },
        CaptureGroup {
            name: "entry-consumer",
            entries: true,
            blocks: consumer,
            instructions: 16,
        },
    ];
    (bank, groups)
}

#[test]
fn capture_word_conditional_move_standalone_profiles_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_CMOV_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let (bank, groups) = word_conditional_bank();
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &bank).unwrap();
    let mut captures = Vec::new();
    for group in groups {
        let artifact = if group.entries {
            compile_entry_region(
                &memory,
                &group
                    .blocks
                    .iter()
                    .map(|block| block.entry)
                    .collect::<Vec<_>>(),
                CompileLimits::default(),
            )
        } else {
            compile_region(&memory, &group.blocks, CompileLimits::default())
        }
        .unwrap();
        assert_eq!(artifact.metadata().instructions, group.instructions);
        assert_eq!(artifact.metadata().blocks, group.blocks.len());
        let wasm = artifact.wasm_bytes(&memory).unwrap();
        assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
        captures.push((format!("{}.wasm", group.name), wasm.to_vec()));
    }
    captures.push(("bank.x86".into(), bank));
    let initial = ring3_engine::process::EngineInstance::new(1, 1)
        .unwrap()
        .arena()
        .to_vec();
    assert_eq!(initial.len(), ring3_engine::abi::arena::ARENA_SIZE);
    captures.push(("initial-arena.bin".into(), initial));
    assert_eq!(captures.len(), 7);
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
        let manifest = format!(
            "{{\"schema_version\":1,\"files\":[{}]}}\n",
            manifest.join(",")
        );
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("capture.json"))
            .unwrap()
            .write_all(manifest.as_bytes())
            .unwrap();
    }
}
