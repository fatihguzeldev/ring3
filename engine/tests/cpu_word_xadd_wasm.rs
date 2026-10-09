use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const LEGACY_CODE: u32 = 0x1000;
const LEGACY_KEY: u64 = 0x1234_5678_cadd_def0;

struct LegacyXaddBank {
    name: &'static str,
    bytes: Vec<u8>,
}

fn legacy_xadd_banks() -> [LegacyXaddBank; 3] {
    let mut bytes = Vec::new();
    let mut dwords = Vec::new();
    let mut words = Vec::new();
    for destination in 0_u8..8 {
        for source in [
            destination,
            destination ^ 4,
            (destination + 1) % 8,
            (destination + 3) % 8,
        ] {
            let modrm = 0xc0 | (source << 3) | destination;
            bytes.extend([0x0f, 0xc0, modrm]);
            dwords.extend([0x0f, 0xc1, modrm]);
            words.extend([0x66, 0x01, modrm]);
        }
    }
    bytes.extend([0xeb, 0]);
    dwords.extend([0xeb, 0]);
    words.extend([0xeb, 0]);
    assert_eq!((bytes.len(), dwords.len(), words.len()), (98, 98, 98));
    [
        LegacyXaddBank {
            name: "legacy-byte-register-xadd",
            bytes,
        },
        LegacyXaddBank {
            name: "legacy-dword-register-xadd",
            bytes: dwords,
        },
        LegacyXaddBank {
            name: "legacy-word-register-add",
            bytes: words,
        },
    ]
}

fn legacy_xadd_engine(bytes: &[u8], key: u64, entries: bool) -> EngineInstance {
    let mut engine = EngineInstance::new(1, key).unwrap();
    assert_eq!(engine.arena().len(), ARENA_SIZE);
    engine.map(LEGACY_CODE, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(LEGACY_CODE, bytes.len() as u32).unwrap();
    engine.protect(LEGACY_CODE, 1, 4).unwrap();
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&LEGACY_CODE.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    }
    engine
}

#[test]
fn capture_legacy_xadd_and_word_add_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_XADD_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let mut captures = Vec::new();
    let mut ordinal = 0;
    let mut next_resident = 1;
    for bank in legacy_xadd_banks() {
        for owner in ["standalone", "replacement", "resident"] {
            for entries in [false, true] {
                let key = LEGACY_KEY + ordinal;
                ordinal += 1;
                let mut engine = legacy_xadd_engine(&bank.bytes, key, entries);
                let before = engine.arena().to_vec();
                let (wasm, receipt) = match owner {
                    "standalone" => {
                        let memory = engine.memory().unwrap();
                        let artifact = if entries {
                            compile_entry_region(
                                memory,
                                &[GuestAddress(LEGACY_CODE)],
                                CompileLimits::default(),
                            )
                        } else {
                            compile_region(
                                memory,
                                &[BlockSpec {
                                    entry: GuestAddress(LEGACY_CODE),
                                    byte_length: bank.bytes.len() as u32,
                                }],
                                CompileLimits::default(),
                            )
                        }
                        .unwrap();
                        assert_eq!(artifact.metadata().instructions, 33);
                        assert_eq!(artifact.metadata().blocks, 1);
                        (artifact.wasm_bytes(memory).unwrap().to_vec(), 0)
                    }
                    "replacement" => {
                        let generation = if entries {
                            engine.compile_entries(1, 0)
                        } else {
                            engine.compile(1)
                        }
                        .unwrap();
                        assert_eq!(generation, 1);
                        engine.guard(key, generation).unwrap();
                        (
                            engine.artifact_bytes().unwrap().to_vec(),
                            u64::from(generation),
                        )
                    }
                    "resident" => {
                        let id = if entries {
                            engine.compile_resident_entries(1, 0)
                        } else {
                            engine.compile_resident(1)
                        }
                        .unwrap()
                        .get();
                        // this test is the only resident allocator in this integration binary.
                        assert_eq!(id, next_resident);
                        next_resident += 1;
                        engine.guard_resident(key, id).unwrap();
                        (engine.resident_bytes(id).unwrap().to_vec(), id)
                    }
                    _ => unreachable!(),
                };
                assert_eq!(engine.generation(), u32::from(owner == "replacement"));
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                let mut code = vec![0; bank.bytes.len()];
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(LEGACY_CODE), &mut code)
                    .unwrap();
                assert_eq!(code, bank.bytes);
                assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
                let profile = if entries { "entry" } else { "explicit" };
                captures.push((
                    format!("{}-{owner}-{profile}.wasm", bank.name),
                    wasm,
                    format!(
                        "\"bank\":\"{}\",\"owner\":\"{owner}\",\"entries\":{entries},\"key_hex\":\"{key:016x}\",\"receipt\":{receipt},\"instructions\":33,\"blocks\":1,\"code_length\":{}",
                        bank.name, bank.bytes.len()
                    ),
                ));
            }
        }
        captures.push((
            format!("{}.x86", bank.name),
            bank.bytes,
            "\"kind\":\"x86\",\"instructions\":33,\"blocks\":1".to_owned(),
        ));
    }
    assert_eq!((ordinal, next_resident, captures.len()), (18, 7, 21));
    if let Some(directory) = directory {
        let mut files = Vec::new();
        for (name, bytes, metadata) in captures {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(&name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            files.push(format!(
                "{{\"file\":\"{name}\",\"bytes\":{},\"code\":{LEGACY_CODE},{metadata}}}",
                bytes.len()
            ));
        }
        let manifest = format!(
            "{{\"schema_version\":1,\"legacy_only\":true,\"modules\":18,\"raw_banks\":3,\"resident_ids_process_global\":true,\"files\":[{}]}}\n",
            files.join(",")
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

fn word_xadd_capture_bank() -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut bank = vec![0xcc; 8192];
    for ordinal in 0_u8..64 {
        let destination = ordinal >> 3;
        let source = ordinal & 7;
        let offset = if ordinal < 32 {
            usize::from(ordinal) * 4
        } else {
            0xf82 + usize::from(ordinal - 32) * 4
        };
        bank[offset..offset + 4].copy_from_slice(&[
            0x66,
            0x0f,
            0xc1,
            0xc0 | (source << 3) | destination,
        ]);
    }
    bank[0x80..0x82].copy_from_slice(&[0xeb, 0xfe]);
    bank[0x1002..0x1004].copy_from_slice(&[0xeb, 0xfe]);
    let pairs = (0_u8..8)
        .map(|alias| (alias, alias))
        .chain([(0, 1), (1, 0), (4, 0), (0, 4)]);
    for (index, (destination, source)) in pairs.enumerate() {
        let offset = 0x400 + index * 4;
        bank[offset..offset + 4].copy_from_slice(&[
            0x66,
            0x0f,
            0xc1,
            0xc0 | (source << 3) | destination,
        ]);
    }
    bank[0x430..0x432].copy_from_slice(&[0xeb, 0xfe]);
    let mut blocks = vec![(0x1400, 50)];
    for (offset, branch) in [(0x800, 0x73), (0x880, 0x72)] {
        let first = [
            0x66, 0x0f, 0xc1, 0xd0, 0x0f, 0x92, 0xc4, 0x0f, 0x90, 0xc1, 0x66, 0x0f, 0xc1, 0xc1,
            branch, 2,
        ];
        let second = [0x66, 0x83, 0xd7, 0, 0x0f, 0x92, 0xc7, 0xeb, 0];
        bank[offset..offset + first.len()].copy_from_slice(&first);
        bank[offset + first.len()..offset + first.len() + 2].copy_from_slice(&[0x0f, 0x0b]);
        let tail = offset + first.len() + 2;
        bank[tail..tail + second.len()].copy_from_slice(&second);
        blocks.extend([
            (LEGACY_CODE + offset as u32, first.len() as u32),
            (LEGACY_CODE + tail as u32, second.len() as u32),
        ]);
    }
    bank[0x1f00..0x1f02].copy_from_slice(&[0x0f, 0x0b]);
    assert_eq!(
        blocks,
        [
            (0x1400, 50),
            (0x1800, 16),
            (0x1812, 9),
            (0x1880, 16),
            (0x1892, 9)
        ]
    );
    (bank, blocks)
}

#[test]
fn capture_word_xadd_inputs_and_standalone_profiles_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_XADD_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let (bank, blocks) = word_xadd_capture_bank();
    let mut default_arena = None;
    let mut modules = Vec::new();
    for entries in [false, true] {
        let mut engine = EngineInstance::new(2, 0x574f_5244_5841_4432).unwrap();
        assert_eq!(engine.arena().len(), ARENA_SIZE);
        if let Some(before) = &default_arena {
            assert_eq!(engine.arena(), before);
        } else {
            default_arena = Some(engine.arena().to_vec());
        }
        engine.map(LEGACY_CODE, 2, 7).unwrap();
        for (page, bytes) in bank.chunks_exact(TRANSFER_SIZE).enumerate() {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]
                .copy_from_slice(bytes);
            engine
                .upload(
                    LEGACY_CODE + (page * TRANSFER_SIZE) as u32,
                    TRANSFER_SIZE as u32,
                )
                .unwrap();
        }
        engine.protect(LEGACY_CODE, 2, 4).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0xa5);
        let before = engine.arena().to_vec();
        let memory = engine.memory().unwrap();
        let artifact = if entries {
            compile_entry_region(
                memory,
                &blocks
                    .iter()
                    .map(|(pc, _)| GuestAddress(*pc))
                    .collect::<Vec<_>>(),
                CompileLimits::default(),
            )
        } else {
            compile_region(
                memory,
                &blocks
                    .iter()
                    .map(|(pc, byte_length)| BlockSpec {
                        entry: GuestAddress(*pc),
                        byte_length: *byte_length,
                    })
                    .collect::<Vec<_>>(),
                CompileLimits::default(),
            )
        }
        .unwrap();
        assert_eq!(artifact.metadata().instructions, 29);
        assert_eq!(artifact.metadata().blocks, 5);
        let wasm = artifact.wasm_bytes(memory).unwrap().to_vec();
        assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.generation(), 0);
        assert_eq!(memory.mapped_pages(), 2);
        let mut physical = vec![0; bank.len()];
        memory
            .fetch(GuestAddress(LEGACY_CODE), &mut physical)
            .unwrap();
        assert_eq!(physical, bank);
        modules.push((
            if entries {
                "entry-profile.wasm"
            } else {
                "profile.wasm"
            },
            wasm,
        ));
    }
    assert_eq!(modules.len(), 2);
    if let Some(directory) = directory {
        let default_arena = default_arena.unwrap();
        let mut files = vec![("bank.x86", bank), ("initial-arena.bin", default_arena)];
        files.extend(modules);
        let mut records = Vec::new();
        for (name, bytes) in files {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            records.push(format!("{{\"file\":\"{name}\",\"bytes\":{}}}", bytes.len()));
        }
        let blocks_json = blocks
            .iter()
            .map(|(pc, length)| format!("[{pc},{length}]"))
            .collect::<Vec<_>>()
            .join(",");
        let groups = [("profile", false), ("entry-profile", true)]
            .map(|(id, entries)| {
                format!(
                    "{{\"id\":\"{id}\",\"entries\":{entries},\"blocks\":[{blocks_json}],\"instructions\":29}}"
                )
            })
            .join(",");
        let manifest = format!(
            "{{\"schema_version\":1,\"raw_bank_bytes\":8192,\"standalone_modules\":2,\"groups\":[{groups}],\"files\":[{}]}}\n",
            records.join(",")
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
