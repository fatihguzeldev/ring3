use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, TRANSFER_SIZE},
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const LEGACY_CODE: u32 = 0x1000;
const LEGACY_KEY: u64 = 0x1234_5678_9abc_def0;

fn legacy_memory_banks() -> [(&'static str, Vec<u8>); 2] {
    let mut conditional = Vec::new();
    for indexed in [false, true] {
        for condition in 0_u8..16 {
            let destination = condition % 8;
            conditional.extend([0x0f, 0x40 + condition]);
            if indexed {
                conditional.extend([0x44 | destination << 3, 0x94, 0x20]);
            } else {
                conditional.push(0x03 | destination << 3);
            }
        }
    }
    conditional.extend([0xeb, 0xfe]);
    let mut moves = Vec::new();
    for indexed in [false, true] {
        for destination in 0_u8..8 {
            moves.extend([0x66, 0x8b]);
            if indexed {
                moves.extend([0x84 | destination << 3, 0x94, 0x20, 0, 0, 0]);
            } else {
                moves.push(0x03 | destination << 3);
            }
        }
    }
    for source in 0_u8..8 {
        moves.extend([0x66, 0x89, 0x03 | source << 3]);
    }
    for index in 0_u8..8 {
        let immediate = 0x8001_u16 + u16::from(index) * 0x0101;
        moves.extend([0x66, 0xc7, 0x43, index * 2]);
        moves.extend(immediate.to_le_bytes());
    }
    moves.extend([0xeb, 0xfe]);
    assert_eq!(conditional.len(), 130);
    assert_eq!(moves.len(), 162);
    [
        ("legacy-dword-memory-cmov", conditional),
        ("legacy-word-memory-mov", moves),
    ]
}

#[test]
fn capture_legacy_memory_conditional_and_word_moves_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_MEMORY_CMOV_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let mut captures = Vec::new();
    for (name, bytes) in legacy_memory_banks() {
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(1, LEGACY_KEY).unwrap();
                assert_eq!(engine.arena().len(), ARENA_SIZE);
                engine.map(LEGACY_CODE, 1, 7).unwrap();
                engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
                    .copy_from_slice(&bytes);
                engine.upload(LEGACY_CODE, bytes.len() as u32).unwrap();
                engine.protect(LEGACY_CODE, 1, 4).unwrap();
                let transfer = &mut engine.arena_mut().unwrap()
                    [TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
                transfer.fill(0xa5);
                transfer[..4].copy_from_slice(&LEGACY_CODE.to_le_bytes());
                if !entries {
                    transfer[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                }
                let before = engine.arena().to_vec();
                let wasm = if resident {
                    let id = if entries {
                        engine.compile_resident_entries(1, 0)
                    } else {
                        engine.compile_resident(1)
                    }
                    .unwrap()
                    .get();
                    engine.guard_resident(LEGACY_KEY, id).unwrap();
                    engine.resident_bytes(id).unwrap().to_vec()
                } else {
                    let generation = if entries {
                        engine.compile_entries(1, 0)
                    } else {
                        engine.compile(1)
                    }
                    .unwrap();
                    engine.guard(LEGACY_KEY, generation).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                };
                assert_eq!(engine.arena(), before);
                assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
                let owner = if resident { "resident" } else { "replacement" };
                let profile = if entries { "entry" } else { "explicit" };
                captures.push((format!("{name}-{owner}-{profile}.wasm"), wasm));
            }
        }
        captures.push((format!("{name}.x86"), bytes));
    }
    assert_eq!(captures.len(), 10);
    if let Some(directory) = directory {
        let mut files = Vec::new();
        for (name, bytes) in captures {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(&name))
                .unwrap()
                .write_all(&bytes)
                .unwrap();
            files.push(format!("{{\"file\":\"{name}\",\"bytes\":{}}}", bytes.len()));
        }
        let manifest = format!(
            "{{\"schema_version\":1,\"legacy_only\":true,\"key_hex\":\"123456789abcdef0\",\"code\":4096,\"files\":[{}]}}\n",
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

struct MemoryInputGroup {
    id: &'static str,
    blocks: Vec<(u32, usize)>,
    instructions: usize,
}

fn append_memory_cmov(bank: &mut [u8], at: &mut usize, cc: u8, destination: u8, tail: &[u8]) {
    let mut bytes = vec![0x66, 0x0f, 0x40 + cc, tail[0] | (destination << 3)];
    bytes.extend_from_slice(&tail[1..]);
    bank[*at..*at + bytes.len()].copy_from_slice(&bytes);
    *at += bytes.len();
}

fn memory_conditional_input_bank() -> (Vec<u8>, Vec<MemoryInputGroup>) {
    let mut bank = vec![0xcc; 4096];
    let mut at = 0;
    for cc in 0_u8..16 {
        let mut tail = vec![0x05];
        tail.extend((0x4000_u32 + u32::from(cc) * 2).to_le_bytes());
        append_memory_cmov(&mut bank, &mut at, cc, cc % 8, &tail);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2, 130);
    at = 0x100;
    let mut aliases = 0;
    for destination in 0_u8..8 {
        let tail = match destination {
            4 => vec![0x04, 0x24],
            5 => vec![0x45, 0],
            _ => vec![destination],
        };
        append_memory_cmov(&mut bank, &mut at, 4, destination, &tail);
        aliases += 1;
        if destination != 4 {
            let mut tail = vec![0x04, (destination << 3) | 5];
            tail.extend(0x4000_u32.to_le_bytes());
            append_memory_cmov(&mut bank, &mut at, 4, destination, &tail);
            let mut tail = vec![
                if destination == 5 { 0x44 } else { 0x04 },
                (destination << 3) | destination,
            ];
            if destination == 5 {
                tail.push(0);
            }
            append_memory_cmov(&mut bank, &mut at, 4, destination, &tail);
            aliases += 2;
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!((aliases, at + 2 - 0x100), (22, 135));
    at = 0x200;
    for (destination, tail) in [
        (3, &[0x83, 0, 0x50, 0, 0][..]),
        (6, &[0x04, 0xf5, 0, 0x40, 0, 0]),
        (1, &[0x84, 0x0b, 0, 0x40, 0, 0]),
        (0, &[0x84, 0x30, 0, 0x40, 0, 0]),
        (5, &[0x84, 0x3d, 0, 0x40, 0, 0]),
        (2, &[0x04, 0x95, 0, 0x40, 0, 0]),
        (3, &[0x43, 0xff]),
        (4, &[0x44, 0x24, 0xff]),
    ] {
        append_memory_cmov(&mut bank, &mut at, 4, destination, tail);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x200, 66);
    at = 0x300;
    for cc in 0_u8..16 {
        bank[at] = 0x90;
        at += 1;
        append_memory_cmov(&mut bank, &mut at, cc, cc % 8, &[0x03]);
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x300, 82);
    for (path, raw) in [
        "6639c8660f4c13660f4d6b020f4cf50f90c40f92c17c02",
        "6639c8660f4213660f436b020f42f50f92c40f90c17202",
    ]
    .into_iter()
    .enumerate()
    {
        let bytes: Vec<u8> = (0..raw.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&raw[index..index + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 23);
        let offset = 0x800 + path * 64;
        bank[offset..offset + 23].copy_from_slice(&bytes);
        bank[offset + 23..offset + 27].copy_from_slice(&[0x0f, 0x0b, 0xeb, 0]);
    }
    bank[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    let groups = vec![
        MemoryInputGroup {
            id: "main",
            blocks: vec![(0x1000, 130), (0x1100, 135), (0x1200, 66)],
            instructions: 49,
        },
        MemoryInputGroup {
            id: "control",
            blocks: vec![
                (0x1300, 82),
                (0x1800, 23),
                (0x1819, 2),
                (0x1840, 23),
                (0x1859, 2),
            ],
            instructions: 49,
        },
        MemoryInputGroup {
            id: "helper",
            blocks: vec![(0x1314, 5)],
            instructions: 2,
        },
        MemoryInputGroup {
            id: "keeper",
            blocks: vec![(0x1803, 4)],
            instructions: 1,
        },
    ];
    (bank, groups)
}

#[test]
fn capture_word_memory_conditional_move_raw_inputs_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_MEMORY_CMOV_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let (bank, groups) = memory_conditional_input_bank();
    let mut data: Vec<u8> = (0..4096)
        .map(|index| (index as u8).wrapping_mul(73).wrapping_add(11))
        .collect();
    data[..4].copy_from_slice(&[1, 0x80, 0x34, 0x12]);
    let initial = EngineInstance::new(1, LEGACY_KEY).unwrap().arena().to_vec();
    assert_eq!((bank.len(), data.len(), initial.len()), (4096, 4096, 4364));
    let mut metadata = Vec::new();
    for group in groups {
        let blocks: Vec<_> = group
            .blocks
            .into_iter()
            .map(|(pc, length)| format!("[{pc},{length}]"))
            .collect();
        metadata.push(format!(
            "{{\"id\":\"{}\",\"blocks\":[{}],\"instructions\":{}}}",
            group.id,
            blocks.join(","),
            group.instructions
        ));
    }
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
