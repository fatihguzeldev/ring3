use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const LEGACY_CODE: u32 = 0x1000;
const LEGACY_KEY: u64 = 0x1234_5678_9abc_def0;

struct LegacyArithmeticBank {
    name: &'static str,
    bytes: Vec<u8>,
    standalone: bool,
}

fn legacy_arithmetic_banks() -> [LegacyArithmeticBank; 4] {
    let mut bytes = Vec::new();
    let mut dwords = Vec::new();
    let mut words = Vec::new();
    let mut checked = Vec::new();
    for destination in 0_u8..8 {
        for opcode in [0x02, 0x2a] {
            bytes.extend([opcode, 0x03 | (destination << 3)]);
            bytes.extend([opcode, 0x44 | (destination << 3), 0x94, 0x20]);
        }
        for opcode in [0x03, 0x2b] {
            dwords.extend([opcode, 0x03 | (destination << 3)]);
            dwords.extend([opcode, 0x44 | (destination << 3), 0x94, 0x20]);
        }
        let other = (destination + 3) % 8;
        words.extend([0x66, 0x01, 0xc0 | (other << 3) | destination]);
        words.extend([0x66, 0x03, 0xc0 | (destination << 3) | other]);
        words.extend([0x66, 0x29, 0xc0 | (other << 3) | destination]);
        words.extend([0x66, 0x2b, 0xc0 | (destination << 3) | other]);

        checked.extend([0x66, 0x8b, 0x03 | (destination << 3)]);
        checked.extend([0x66, 0x8b, 0x44 | (destination << 3), 0x94, 0x20]);
        checked.extend([0x66, 0x39, 0x03 | (destination << 3)]);
        checked.extend([0x66, 0x85, 0x44 | (destination << 3), 0x94, 0x20]);
    }
    for bank in [&mut bytes, &mut dwords, &mut words, &mut checked] {
        bank.extend([0xeb, 0xfe]);
    }
    assert_eq!(
        (bytes.len(), dwords.len(), words.len(), checked.len()),
        (98, 98, 98, 130)
    );
    [
        LegacyArithmeticBank {
            name: "legacy-byte-read-arithmetic",
            bytes,
            standalone: false,
        },
        LegacyArithmeticBank {
            name: "legacy-dword-read-arithmetic",
            bytes: dwords,
            standalone: false,
        },
        LegacyArithmeticBank {
            name: "legacy-word-register-add-sub",
            bytes: words,
            standalone: true,
        },
        LegacyArithmeticBank {
            name: "legacy-word-checked-read",
            bytes: checked,
            standalone: false,
        },
    ]
}

fn legacy_arithmetic_engine(bytes: &[u8], key: u64, entries: bool) -> EngineInstance {
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
fn capture_legacy_arithmetic_and_checked_word_reads_without_guest_execution() {
    let directory =
        std::env::var_os("RING3_WORD_MEMORY_LOGICAL_STORE_CAPTURE_DIR").map(PathBuf::from);
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
    for bank in legacy_arithmetic_banks() {
        for owner in ["replacement", "resident", "standalone"] {
            if owner == "standalone" && !bank.standalone {
                continue;
            }
            for entries in [false, true] {
                let key = LEGACY_KEY + ordinal;
                ordinal += 1;
                let mut engine = legacy_arithmetic_engine(&bank.bytes, key, entries);
                let before = engine.arena().to_vec();
                let (wasm, receipt) = match owner {
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
    assert_eq!((ordinal, next_resident, captures.len()), (18, 9, 22));
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
            "{{\"schema_version\":1,\"legacy_only\":true,\"modules\":18,\"raw_banks\":4,\"resident_ids_process_global\":true,\"files\":[{}]}}\n",
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

struct LogicalStoreInputGroup {
    id: &'static str,
    blocks: Vec<(u32, usize)>,
    instructions: usize,
}

fn append_word_memory_logical_store(
    bank: &mut [u8],
    at: &mut usize,
    opcode: u8,
    source: u8,
    tail: &[u8],
) {
    let mut bytes = vec![0x66, opcode, tail[0] | (source << 3)];
    bytes.extend_from_slice(&tail[1..]);
    bank[*at..*at + bytes.len()].copy_from_slice(&bytes);
    *at += bytes.len();
}

fn word_memory_logical_store_input_bank() -> (Vec<u8>, Vec<LogicalStoreInputGroup>) {
    let mut bank = vec![0xcc; 4096];
    let mut at = 0;
    for (kind, opcode) in [0x21, 0x09, 0x31].into_iter().enumerate() {
        for source in 0_u8..8 {
            let address = 0x4000_u32 + 2 * (kind as u32 * 8 + u32::from(source));
            let mut tail = vec![0x05];
            tail.extend(address.to_le_bytes());
            append_word_memory_logical_store(&mut bank, &mut at, opcode, source, &tail);
        }
    }
    for opcode in [0x21, 0x09, 0x31] {
        for (source, tail) in [
            (3, &[0x83, 0x80, 0x51, 0, 0][..]),
            (6, &[4, 0xf5, 0x80, 0x41, 0, 0]),
            (1, &[0x84, 0x0b, 0x80, 0x41, 0, 0]),
            (0, &[0x84, 0x30, 0x80, 0x41, 0, 0]),
            (5, &[0x84, 0x3d, 0x80, 0x41, 0, 0]),
            (2, &[4, 0x95, 0x80, 0x41, 0, 0]),
            (3, &[0x43, 0xff]),
            (4, &[0x44, 0x24, 0xff]),
        ] {
            append_word_memory_logical_store(&mut bank, &mut at, opcode, source, tail);
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2, 338);
    at = 0x200;
    for opcode in [0x21, 0x09, 0x31] {
        for register in 0_u8..8 {
            let tail = match register {
                4 => vec![4, 0x24],
                5 => vec![0x45, 0],
                _ => vec![register],
            };
            append_word_memory_logical_store(&mut bank, &mut at, opcode, register, &tail);
        }
        for register in 0_u8..8 {
            if register != 4 {
                let mut tail = vec![4, (register << 3) | 5];
                tail.extend(0x4180_u32.to_le_bytes());
                append_word_memory_logical_store(&mut bank, &mut at, opcode, register, &tail);
            }
        }
        append_word_memory_logical_store(&mut bank, &mut at, opcode, 3, &[4, 0x1b]);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x200, 260);
    at = 0x600;
    for opcode in [0x21, 0x09, 0x31] {
        bank[at] = 0x90;
        at += 1;
        append_word_memory_logical_store(&mut bank, &mut at, opcode, 0, &[3]);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x600, 14);
    let mut control = vec![(0x1600, 14)];
    for path in 0..2 {
        let offset = 0x800 + path * 64;
        let value: u16 = if path == 0 { 0 } else { 0xffff };
        let prefix = [
            0x66,
            0xb8,
            value as u8,
            (value >> 8) as u8,
            0x66,
            0xbf,
            0xff,
            0xff,
            0x66,
            0x83,
            0xef,
            1,
        ];
        let body = [
            0x66,
            if path == 0 { 0x21 } else { 0x31 },
            3,
            0x66,
            0x8b,
            0x0b,
            0x0f,
            0x92,
            0xc3,
            0x0f,
            0x90,
            0xc2,
            0x0f,
            0x94,
            0xc4,
            if path == 0 { 0x74 } else { 0x75 },
            2,
        ];
        bank[offset..offset + 12].copy_from_slice(&prefix);
        bank[offset + 12..offset + 29].copy_from_slice(&body);
        bank[offset + 29..offset + 40]
            .copy_from_slice(&[0x0f, 0x0b, 0x66, 0x83, 0xd7, 0, 0x0f, 0x92, 0xc7, 0xeb, 0]);
        control.extend([
            (0x1000 + offset as u32, 29),
            (0x1000 + offset as u32 + 31, 9),
        ]);
    }
    for path in 0..2 {
        let offset = 0xa00 + path * 64;
        let bytes = [
            0x66,
            if path == 0 { 0x09 } else { 0x21 },
            3,
            0x66,
            0x8b,
            0x1b,
            0x66,
            0x31,
            0x13,
            0x0f,
            0x94,
            0xc3,
            0x66,
            if path == 0 { 0x21 } else { 0x09 },
            0x33,
            0xeb,
            0,
        ];
        bank[offset..offset + bytes.len()].copy_from_slice(&bytes);
        control.push((0x1000 + offset as u32, bytes.len()));
    }
    for (offset, opcode) in [(0xd00, 0x31), (0xd40, 0x09)] {
        let mut bytes = vec![0x66, opcode, 5];
        bytes.extend((0x1000_u32 + offset as u32).to_le_bytes());
        bytes.extend([0x90, 0xeb, 0]);
        assert_eq!(bytes.len(), 10);
        bank[offset..offset + bytes.len()].copy_from_slice(&bytes);
    }
    bank[0xe00..0xe03].copy_from_slice(&[0x90, 0xeb, 0]);
    bank[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    let groups = vec![
        LogicalStoreInputGroup {
            id: "main",
            blocks: vec![(0x1000, 338)],
            instructions: 49,
        },
        LogicalStoreInputGroup {
            id: "alias",
            blocks: vec![(0x1200, 260)],
            instructions: 49,
        },
        LogicalStoreInputGroup {
            id: "control",
            blocks: control,
            instructions: 43,
        },
        LogicalStoreInputGroup {
            id: "helper",
            blocks: vec![(0x1600, 4)],
            instructions: 2,
        },
        LogicalStoreInputGroup {
            id: "changed",
            blocks: vec![(0x1d00, 10)],
            instructions: 3,
        },
        LogicalStoreInputGroup {
            id: "changed-successor",
            blocks: vec![(0x1d07, 3)],
            instructions: 2,
        },
        LogicalStoreInputGroup {
            id: "equal",
            blocks: vec![(0x1d40, 10)],
            instructions: 3,
        },
        LogicalStoreInputGroup {
            id: "equal-successor",
            blocks: vec![(0x1d47, 3)],
            instructions: 2,
        },
        LogicalStoreInputGroup {
            id: "other-code",
            blocks: vec![(0x1e00, 3)],
            instructions: 2,
        },
        LogicalStoreInputGroup {
            id: "unrelated",
            blocks: vec![(0x4e00, 3)],
            instructions: 2,
        },
    ];
    for group in &groups {
        assert!(group.blocks.len() <= 8 && group.instructions <= 64);
        for &(pc, size) in &group.blocks {
            if group.id == "unrelated" {
                assert_eq!((pc, size), (0x4e00, 3));
            } else {
                assert!(pc >= 0x1000 && pc as usize + size <= 0x2000);
            }
        }
    }
    (bank, groups)
}

#[test]
fn capture_word_memory_logical_store_raw_inputs_without_guest_execution() {
    let directory =
        std::env::var_os("RING3_WORD_MEMORY_LOGICAL_STORE_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let (bank, groups) = word_memory_logical_store_input_bank();
    let mut data: Vec<u8> = (0..4096)
        .map(|i| (i as u8).wrapping_mul(73).wrapping_add(11))
        .collect();
    for (index, value) in [
        0_u16, 0xffff, 0x8000, 0x7fff, 0xff, 0xaaaa, 0x1234, 0xffff, 0, 0, 0x7fff, 0x8000, 0xfe,
        0xaaaa, 0x1234, 0xffff, 0, 0xffff, 0x8000, 0x7fff, 0xff, 0xaaaa, 0x1234, 0xffff,
    ]
    .into_iter()
    .enumerate()
    {
        data[2 * index..2 * index + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (0x100, 0x4100_u16),
        (0x102, 0x0f0f),
        (0x180, 0x8001),
        (0x200, 0x4200),
        (0x202, 0x4202),
        (0x300, 0xffff),
        (0x302, 0x8000),
    ] {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    data[0xe00..0xe03].copy_from_slice(&[0x90, 0xeb, 0]);
    let initial = EngineInstance::new(1, LEGACY_KEY).unwrap().arena().to_vec();
    assert_eq!(
        (bank.len(), data.len(), initial.len()),
        (4096, 4096, ARENA_SIZE)
    );
    let metadata: Vec<_> = groups
        .into_iter()
        .map(|group| {
            let blocks: Vec<_> = group
                .blocks
                .into_iter()
                .map(|(pc, size)| format!("[{pc},{size}]"))
                .collect();
            format!(
                "{{\"id\":\"{}\",\"blocks\":[{}],\"instructions\":{}}}",
                group.id,
                blocks.join(","),
                group.instructions
            )
        })
        .collect();
    if let Some(directory) = directory {
        let mut files = Vec::new();
        for (name, bytes) in [
            ("bank.x86", bank),
            ("data.bin", data),
            ("initial-arena.bin", initial),
        ] {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            files.push(format!("{{\"file\":\"{name}\",\"bytes\":{}}}", bytes.len()));
        }
        let manifest = format!(
            "{{\"schema_version\":1,\"raw_inputs_only\":true,\"groups\":[{}],\"files\":[{}]}}\n",
            metadata.join(","),
            files.join(",")
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
