use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const LEGACY_CODE: u32 = 0x1000;
const LEGACY_KEY: u64 = 0x1234_5678_9abc_def0;

struct LegacyDoubleShiftBank {
    name: &'static str,
    bytes: Vec<u8>,
    standalone: bool,
}

fn legacy_double_shift_banks() -> [LegacyDoubleShiftBank; 3] {
    let double_counts = [0, 1, 16, 17, 0x66, 0x67, 0xf0, 0xf3];
    let word_counts = [0, 1, 16, 17, 31, 0x66, 0x67, 255];
    let mut registers = Vec::new();
    let mut memory = Vec::new();
    let mut words = Vec::new();
    for alias in 0_u8..8 {
        let source = (alias + 3) % 8;
        for opcode in [0xa4, 0xa5, 0xac, 0xad] {
            registers.extend([0x0f, opcode, 0xc0 | (source << 3) | alias]);
            if matches!(opcode, 0xa4 | 0xac) {
                registers.push(double_counts[usize::from(alias)]);
            }
        }

        memory.extend([
            0x0f,
            0xa4,
            0x03 | (alias << 3),
            double_counts[usize::from(alias)],
        ]);
        memory.extend([0x0f, 0xa5, 0x44 | (alias << 3), 0x94, 0x20]);
        memory.extend([
            0x0f,
            0xac,
            0x43 | (alias << 3),
            0x20,
            double_counts[usize::from(alias)],
        ]);
        memory.extend([0x0f, 0xad, 0x04 | (alias << 3), 0x94]);

        words.extend([0x66, 0xd1, 0xe0 | alias]);
        words.extend([0x66, 0xc1, 0xe8 | alias, word_counts[usize::from(alias)]]);
        words.extend([0x66, 0xd3, 0xf8 | alias]);
        let opcode = [0xd1, 0xc1, 0xd3][usize::from(alias % 3)];
        let extension = 2 + (alias % 2);
        words.extend([0x66, opcode, 0xc0 | (extension << 3) | alias]);
        if opcode == 0xc1 {
            words.push(word_counts[usize::from(alias)]);
        }
    }
    registers.extend([0xeb, 0xfe]);
    memory.extend([0xeb, 0xfe]);
    words.extend([0xeb, 0xfe]);
    assert_eq!(
        (registers.len(), memory.len(), words.len()),
        (114, 146, 109)
    );
    [
        LegacyDoubleShiftBank {
            name: "legacy-dword-register-double-shift",
            bytes: registers,
            standalone: true,
        },
        LegacyDoubleShiftBank {
            name: "legacy-dword-memory-double-shift",
            bytes: memory,
            standalone: false,
        },
        LegacyDoubleShiftBank {
            name: "legacy-word-shift-carry",
            bytes: words,
            standalone: true,
        },
    ]
}

fn legacy_double_shift_engine(bytes: &[u8], key: u64, entries: bool) -> EngineInstance {
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
fn capture_legacy_double_shifts_and_word_shift_carry_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_DOUBLE_SHIFT_CAPTURE_DIR").map(PathBuf::from);
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
    for bank in legacy_double_shift_banks() {
        for owner in ["standalone", "replacement", "resident"] {
            if owner == "standalone" && !bank.standalone {
                continue;
            }
            for entries in [false, true] {
                let key = LEGACY_KEY + ordinal;
                ordinal += 1;
                let mut engine = legacy_double_shift_engine(&bank.bytes, key, entries);
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
    assert_eq!((ordinal, next_resident, captures.len()), (16, 7, 19));
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
            "{{\"schema_version\":1,\"legacy_only\":true,\"modules\":16,\"raw_banks\":3,\"resident_ids_process_global\":true,\"files\":[{}]}}\n",
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

fn word_double_shift_capture_instruction(
    opcode: u8,
    destination: u8,
    source: u8,
    immediate: Option<u8>,
) -> Vec<u8> {
    let mut bytes = vec![0x66, 0x0f, opcode, 0xc0 | (source << 3) | destination];
    if let Some(value) = immediate {
        bytes.push(value);
    }
    bytes
}

fn word_double_shift_capture_cohorts(
    bank: &mut [u8],
    cursor: &mut usize,
    instructions: &[Vec<u8>],
) {
    for cohort in instructions.chunks(60) {
        for bytes in cohort {
            bank[*cursor..*cursor + bytes.len()].copy_from_slice(bytes);
            *cursor += bytes.len();
        }
        bank[*cursor..*cursor + 2].copy_from_slice(&[0xeb, 0xfe]);
        *cursor += 2;
    }
}

fn word_double_shift_capture_bank() -> (Vec<u8>, Vec<(u32, u32)>) {
    let mut bank = vec![0xcc; 8192];
    let mut raw = Vec::new();
    for opcode in [0xa4, 0xac] {
        for count in 0_u8..=255 {
            raw.push(word_double_shift_capture_instruction(
                opcode,
                0,
                2,
                Some(count),
            ));
        }
    }
    for opcode in [0xa5, 0xad] {
        raw.push(word_double_shift_capture_instruction(opcode, 0, 2, None));
    }
    assert_eq!(raw.len(), 514);
    let mut aliases = Vec::new();
    for (immediate, cl) in [(0xa4, 0xa5), (0xac, 0xad)] {
        for destination in 0..8 {
            for source in 0..8 {
                for count in [1, 16] {
                    aliases.push(word_double_shift_capture_instruction(
                        immediate,
                        destination,
                        source,
                        Some(count),
                    ));
                }
                aliases.push(word_double_shift_capture_instruction(
                    cl,
                    destination,
                    source,
                    None,
                ));
            }
        }
    }
    assert_eq!(aliases.len(), 384);
    let mut cursor = 0;
    word_double_shift_capture_cohorts(&mut bank, &mut cursor, &raw);
    word_double_shift_capture_cohorts(&mut bank, &mut cursor, &aliases);
    assert_eq!(cursor, 4392);

    cursor = 0x1400;
    let mut profile = Vec::new();
    for (immediate, cl) in [(0xa4, 0xa5), (0xac, 0xad)] {
        for count in [0, 1, 16, 17, 31] {
            profile.push(word_double_shift_capture_instruction(
                immediate,
                0,
                2,
                Some(count),
            ));
        }
        profile.push(word_double_shift_capture_instruction(cl, 0, 2, None));
    }
    assert_eq!(raw.len() + aliases.len() + profile.len(), 910);
    word_double_shift_capture_cohorts(&mut bank, &mut cursor, &profile);
    assert_eq!(cursor, 0x143c);
    let mut blocks = vec![(0x2400, 60)];
    for (index, opcode) in [0xa4, 0xac].into_iter().enumerate() {
        cursor = 0x1800 + index * 0x80;
        let start = cursor;
        let pieces = [
            vec![
                0x66, 0x0f, opcode, 0xd0, 17, 0x0f, 0x94, 0xc4, 0x0f, 0x9a, 0xc1, 0x75, 2,
            ],
            vec![0x7b, 2],
            vec![0x66, 0x83, 0xd7, 0, 0x0f, 0x92, 0xc7, 0xeb, 0],
        ];
        for (piece, bytes) in pieces.into_iter().enumerate() {
            bank[cursor..cursor + bytes.len()].copy_from_slice(&bytes);
            blocks.push((LEGACY_CODE + cursor as u32, bytes.len() as u32));
            cursor += bytes.len();
            if piece != 2 {
                bank[cursor..cursor + 2].copy_from_slice(&[0x0f, 0x0b]);
                cursor += 2;
            }
        }
        assert_eq!(cursor - start, 28);
    }
    bank[0x1f00..0x1f02].copy_from_slice(&[0x0f, 0x0b]);
    assert_eq!(
        blocks,
        [
            (0x2400, 60),
            (0x2800, 13),
            (0x280f, 2),
            (0x2813, 9),
            (0x2880, 13),
            (0x288f, 2),
            (0x2893, 9),
        ]
    );
    (bank, blocks)
}

#[test]
fn capture_word_double_shift_inputs_and_pure_modules_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_DOUBLE_SHIFT_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let (bank, blocks) = word_double_shift_capture_bank();
    let mut default_arena = None;
    let mut modules = Vec::new();
    for entries in [false, true] {
        let mut engine = EngineInstance::new(2, 0x5744_5348_4946_5401).unwrap();
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
        assert_eq!(artifact.metadata().blocks, 7);
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
