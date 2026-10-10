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
    let directory = std::env::var_os("RING3_WORD_MEMORY_LOGICAL_CAPTURE_DIR").map(PathBuf::from);
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

struct LogicalInputGroup {
    id: &'static str,
    blocks: Vec<(u32, usize)>,
    instructions: usize,
}

fn append_word_memory_logical(
    bank: &mut [u8],
    at: &mut usize,
    opcode: u8,
    destination: u8,
    tail: &[u8],
) {
    let mut bytes = vec![0x66, opcode, tail[0] | (destination << 3)];
    bytes.extend_from_slice(&tail[1..]);
    bank[*at..*at + bytes.len()].copy_from_slice(&bytes);
    *at += bytes.len();
}

fn word_memory_logical_input_bank() -> (Vec<u8>, Vec<LogicalInputGroup>) {
    let mut bank = vec![0xcc; 4096];
    let mut at = 0;
    for (kind, opcode) in [0x23, 0x0b, 0x33].into_iter().enumerate() {
        for destination in 0_u8..8 {
            let address = 0x4000_u32 + 2 * (kind as u32 * 8 + u32::from(destination));
            let mut tail = vec![0x05];
            tail.extend(address.to_le_bytes());
            append_word_memory_logical(&mut bank, &mut at, opcode, destination, &tail);
        }
    }
    for opcode in [0x23, 0x0b, 0x33] {
        for (destination, tail) in [
            (3, &[0x83, 0x80, 0x51, 0, 0][..]),
            (6, &[4, 0xf5, 0x80, 0x41, 0, 0]),
            (1, &[0x84, 0x0b, 0x80, 0x41, 0, 0]),
            (0, &[0x84, 0x30, 0x80, 0x41, 0, 0]),
            (5, &[0x84, 0x3d, 0x80, 0x41, 0, 0]),
            (2, &[4, 0x95, 0x80, 0x41, 0, 0]),
            (3, &[0x43, 0xff]),
            (4, &[0x44, 0x24, 0xff]),
        ] {
            append_word_memory_logical(&mut bank, &mut at, opcode, destination, tail);
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2, 338);
    at = 0x200;
    for opcode in [0x23, 0x0b, 0x33] {
        for register in 0_u8..8 {
            let tail = match register {
                4 => vec![4, 0x24],
                5 => vec![0x45, 0],
                _ => vec![register],
            };
            append_word_memory_logical(&mut bank, &mut at, opcode, register, &tail);
            if register != 4 {
                let mut tail = vec![4, (register << 3) | 5];
                tail.extend(0x4180_u32.to_le_bytes());
                append_word_memory_logical(&mut bank, &mut at, opcode, register, &tail);
                if register != 7 {
                    let mut tail = vec![
                        if register == 5 { 0x44 } else { 4 },
                        (register << 3) | register,
                    ];
                    if register == 5 {
                        tail.push(0);
                    }
                    append_word_memory_logical(&mut bank, &mut at, opcode, register, &tail);
                }
            }
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x200, 323);
    at = 0x600;
    for opcode in [0x23, 0x0b, 0x33] {
        bank[at] = 0x90;
        at += 1;
        append_word_memory_logical(&mut bank, &mut at, opcode, 0, &[3]);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x600, 14);
    let mut consumer_blocks = Vec::new();
    for path in 0..3 {
        let offset = 0x800 + path * 64;
        let value: u16 = [0, 0x7fff, 0xff][path];
        let prefix = [
            0x66,
            0xb8,
            value as u8,
            (value >> 8) as u8,
            0x66,
            0xbf,
            0,
            0,
            0x66,
            0x83,
            0xef,
            1,
        ];
        bank[offset..offset + 12].copy_from_slice(&prefix);
        let body = [
            0x66,
            [0x23, 0x0b, 0x33][path],
            3,
            0x0f,
            0x92,
            0xc3,
            0x0f,
            0x90,
            0xc1,
            0x0f,
            0x94,
            0xc2,
            0x0f,
            0x98,
            0xc6,
            0x0f,
            0x9a,
            0xc5,
            [0x74, 0x78, 0x7b][path],
            2,
        ];
        bank[offset + 12..offset + 32].copy_from_slice(&body);
        bank[offset + 32..offset + 43].copy_from_slice(&[
            0x0f,
            0x0b,
            0x66,
            0x83,
            if path == 1 { 0xdf } else { 0xd7 },
            0,
            0x0f,
            0x92,
            0xc7,
            0xeb,
            0,
        ]);
        consumer_blocks.push((0x1000 + offset as u32, 32));
        consumer_blocks.push((0x1000 + offset as u32 + 34, 9));
    }
    let mut next_blocks = Vec::new();
    for path in 0..2 {
        let offset = 0xa00 + path * 64;
        let mut bytes = vec![0x66, if path == 0 { 0x0b } else { 0x23 }, 0x9b];
        bytes.extend(0x5bfc0000_u32.to_le_bytes());
        bytes.extend([0x66, 0x33, 0x83]);
        bytes.extend(0x5bfc0000_u32.to_le_bytes());
        bytes.extend([
            0x0f,
            if path == 0 { 0x9b } else { 0x94 },
            0xc4,
            if path == 0 { 0x7b } else { 0x74 },
            2,
        ]);
        assert_eq!(bytes.len(), 19);
        bank[offset..offset + 19].copy_from_slice(&bytes);
        bank[offset + 19..offset + 23].copy_from_slice(&[0x0f, 0x0b, 0xeb, 0]);
        next_blocks.extend([
            (0x1000 + offset as u32, 19),
            (0x1000 + offset as u32 + 21, 2),
        ]);
    }
    bank[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    let mut control_a = vec![(0x1600, 14)];
    control_a.extend(consumer_blocks);
    let groups = vec![
        LogicalInputGroup {
            id: "main",
            blocks: vec![(0x1000, 338)],
            instructions: 49,
        },
        LogicalInputGroup {
            id: "alias",
            blocks: vec![(0x1200, 323)],
            instructions: 64,
        },
        LogicalInputGroup {
            id: "control-a",
            blocks: control_a,
            instructions: 46,
        },
        LogicalInputGroup {
            id: "control-b",
            blocks: next_blocks,
            instructions: 10,
        },
        LogicalInputGroup {
            id: "helper",
            blocks: vec![(0x1600, 4)],
            instructions: 2,
        },
        LogicalInputGroup {
            id: "keeper",
            blocks: vec![(0x1601, 3)],
            instructions: 1,
        },
    ];
    for group in &groups {
        assert!(group.blocks.len() <= 8 && group.instructions <= 64);
        for &(pc, size) in &group.blocks {
            assert!(pc >= 0x1000 && pc as usize + size <= 0x2000);
        }
    }
    (bank, groups)
}

#[test]
fn capture_word_memory_logical_raw_inputs_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_MEMORY_LOGICAL_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
        assert!(fs::read_dir(directory).unwrap().next().is_none());
    }
    let (bank, groups) = word_memory_logical_input_bank();
    let mut data: Vec<u8> = (0..4096)
        .map(|i| (i as u8).wrapping_mul(73).wrapping_add(11))
        .collect();
    for (index, value) in [
        0xffff_u16, 0, 0xffff, 0xffff, 0xff, 0x5555, 0x0f0f, 0xffff, 0, 1, 0x8000, 0, 1, 0x5555,
        0xf0, 0, 0, 0xffff, 0, 0xffff, 1, 0x5555, 0xedcb, 0,
    ]
    .into_iter()
    .enumerate()
    {
        data[2 * index..2 * index + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (0x100, 0x4102_u16),
        (0x102, 0x0f0f),
        (0x180, 0x8001),
        (0x200, 0x4200),
        (0x202, 0xf0f0),
        (0x300, 0xffff),
        (0x302, 0x8000),
        (0x304, 1),
    ] {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
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
