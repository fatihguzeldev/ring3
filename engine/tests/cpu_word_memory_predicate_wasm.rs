use ring3_engine::{
    abi::arena::{ARENA_SIZE, TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    memory::GuestAddress,
    process::EngineInstance,
};
use std::{fs, fs::OpenOptions, io::Write, path::PathBuf};

const LEGACY_CODE: u32 = 0x1000;
const LEGACY_KEY: u64 = 0x1234_5678_9abc_def0;

struct LegacyPredicateBank {
    name: &'static str,
    bytes: Vec<u8>,
    instructions: usize,
    standalone: bool,
}

fn legacy_predicate_banks() -> [LegacyPredicateBank; 3] {
    let mut registers = Vec::new();
    let mut memory = Vec::new();
    for alias in 0_u8..8 {
        let other = (alias + 3) % 8;
        let immediate = 0x8001_u16 + u16::from(alias) * 0x0101;
        registers.extend([0x66, 0x39, 0xc0 | (other << 3) | alias]);
        registers.extend([0x66, 0x3b, 0xc0 | (alias << 3) | other]);
        registers.extend([0x66, 0x81, 0xf8 | alias]);
        registers.extend(immediate.to_le_bytes());
        registers.extend([0x66, 0x83, 0xf8 | alias, 0x80 | alias]);
        registers.extend([0x66, 0x85, 0xc0 | (other << 3) | alias]);
        registers.extend([0x66, 0xf7, 0xc0 | alias]);
        registers.extend((0x6766_u16 ^ (u16::from(alias) * 0x0101)).to_le_bytes());

        memory.extend([0x39, 0x03 | (alias << 3)]);
        memory.extend([0x3b, 0x43 | (alias << 3), 0x20]);
        memory.extend([0x81, 0x3b]);
        memory.extend((0x9234_5678_u32 + u32::from(alias) * 0x0101).to_le_bytes());
        memory.extend([0x83, 0x7b, 0x20, 0x80 | alias]);
        memory.extend([0x85, 0x04 | (alias << 3), 0x94]);
        memory.extend([0xf7, 0x03]);
        memory.extend((0xf0f3_6766_u32 ^ (u32::from(alias) * 0x0101)).to_le_bytes());
    }
    registers.extend([0x66, 0x3d, 0x67, 0x66, 0x66, 0xa9, 0xf3, 0xf0]);
    registers.extend([0xeb, 0xfe]);
    memory.extend([0xeb, 0xfe]);
    let mut conditional = Vec::new();
    for indexed in [false, true] {
        for condition in 0_u8..16 {
            let destination = condition % 8;
            conditional.extend([0x66, 0x0f, 0x40 + condition]);
            if indexed {
                conditional.extend([0x44 | (destination << 3), 0x94, 0x20]);
            } else {
                conditional.push(0x03 | (destination << 3));
            }
        }
    }
    conditional.extend([0xeb, 0xfe]);
    assert_eq!(
        (registers.len(), memory.len(), conditional.len()),
        (194, 194, 162)
    );
    [
        LegacyPredicateBank {
            name: "legacy-word-register-predicate",
            bytes: registers,
            instructions: 51,
            standalone: true,
        },
        LegacyPredicateBank {
            name: "legacy-dword-memory-predicate",
            bytes: memory,
            instructions: 49,
            standalone: false,
        },
        LegacyPredicateBank {
            name: "legacy-word-memory-cmov",
            bytes: conditional,
            instructions: 33,
            standalone: false,
        },
    ]
}

fn legacy_predicate_engine(bytes: &[u8], key: u64, entries: bool) -> EngineInstance {
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
fn capture_legacy_predicates_and_memory_conditional_moves_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_MEMORY_PREDICATE_CAPTURE_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let mut captures = Vec::new();
    let mut ordinal = 0;
    for bank in legacy_predicate_banks() {
        for resident in [false, true] {
            for entries in [false, true] {
                let key = LEGACY_KEY + ordinal;
                ordinal += 1;
                let mut engine = legacy_predicate_engine(&bank.bytes, key, entries);
                let before = engine.arena().to_vec();
                let wasm = if resident {
                    let id = if entries {
                        engine.compile_resident_entries(1, 0)
                    } else {
                        engine.compile_resident(1)
                    }
                    .unwrap()
                    .get();
                    engine.guard_resident(key, id).unwrap();
                    assert_eq!(engine.generation(), 0);
                    engine.resident_bytes(id).unwrap().to_vec()
                } else {
                    let generation = if entries {
                        engine.compile_entries(1, 0)
                    } else {
                        engine.compile(1)
                    }
                    .unwrap();
                    assert_eq!(generation, 1);
                    engine.guard(key, generation).unwrap();
                    engine.artifact_bytes().unwrap().to_vec()
                };
                assert_eq!(engine.arena(), before);
                assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
                let owner = if resident { "resident" } else { "replacement" };
                let profile = if entries { "entry" } else { "explicit" };
                captures.push((
                    format!("{}-{owner}-{profile}.wasm", bank.name),
                    wasm,
                    format!(
                        "\"bank\":\"{}\",\"owner\":\"{owner}\",\"entries\":{entries},\"key_hex\":\"{key:016x}\",\"instructions\":{}",
                        bank.name, bank.instructions
                    ),
                ));
            }
        }
        if bank.standalone {
            for entries in [false, true] {
                let key = LEGACY_KEY + ordinal;
                ordinal += 1;
                let engine = legacy_predicate_engine(&bank.bytes, key, entries);
                let before = engine.arena().to_vec();
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
                assert_eq!(artifact.metadata().instructions, bank.instructions);
                assert_eq!(artifact.metadata().blocks, 1);
                let wasm = artifact.wasm_bytes(memory).unwrap().to_vec();
                assert_eq!(engine.arena(), before);
                assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
                let profile = if entries { "entry" } else { "explicit" };
                captures.push((
                    format!("{}-standalone-{profile}.wasm", bank.name),
                    wasm,
                    format!(
                        "\"bank\":\"{}\",\"owner\":\"standalone\",\"entries\":{entries},\"key_hex\":\"{key:016x}\",\"instructions\":{}",
                        bank.name, bank.instructions
                    ),
                ));
            }
        }
        captures.push((
            format!("{}.x86", bank.name),
            bank.bytes,
            format!("\"kind\":\"x86\",\"instructions\":{}", bank.instructions),
        ));
    }
    assert_eq!((ordinal, captures.len()), (14, 17));
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
            "{{\"schema_version\":1,\"legacy_only\":true,\"modules\":14,\"raw_banks\":3,\"files\":[{}]}}\n",
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

struct PredicateInputGroup {
    id: &'static str,
    blocks: Vec<(u32, usize)>,
    instructions: usize,
}

fn append_memory_predicate(
    bank: &mut [u8],
    at: &mut usize,
    form: u8,
    register: u8,
    tail: &[u8],
    immediate: u16,
) {
    let (opcode, field) = match form {
        0 => (0x39, register),
        1 => (0x3b, register),
        2 => (0x81, 7),
        3 => (0x83, 7),
        4 => (0x85, register),
        5 => (0xf7, 0),
        _ => unreachable!(),
    };
    let mut bytes = vec![0x66, opcode, tail[0] | (field << 3)];
    bytes.extend_from_slice(&tail[1..]);
    match form {
        2 | 5 => bytes.extend(immediate.to_le_bytes()),
        3 => bytes.push(immediate as u8),
        _ => {}
    }
    bank[*at..*at + bytes.len()].copy_from_slice(&bytes);
    *at += bytes.len();
}

fn predicate_input_pairs(form: u8) -> [(u16, u16); 8] {
    if form >= 4 {
        [
            (0, 0xffff),
            (0x8000, 0x8000),
            (0x7fff, 0xffff),
            (0xff, 0x0f),
            (0x101, 0x101),
            (0xffff, 0xaaaa),
            (0x5555, 0xaaaa),
            (0x1234, 0xff),
        ]
    } else {
        [
            (0, 0),
            (0, 1),
            (0x8000, 1),
            (0x7fff, 0xffff),
            (0x10, 1),
            (0x101, 1),
            (0xffff, 0xffff),
            (0x8000, 0x7fff),
        ]
    }
}

fn memory_predicate_input_bank() -> (Vec<u8>, Vec<PredicateInputGroup>) {
    let mut bank = vec![0xcc; 4096];
    let mut at = 0;
    for form in 0_u8..6 {
        for (index, (_, right)) in predicate_input_pairs(form).into_iter().enumerate() {
            let address = 0x4000_u32 + 2 * (u32::from(form) * 8 + index as u32);
            let mut tail = vec![0x05];
            tail.extend(address.to_le_bytes());
            let immediate = if form == 3 {
                [0, 1, 0x7f, 0x80, 0xff, 0x10, 0x0f, 0x55][index]
            } else {
                right
            };
            append_memory_predicate(&mut bank, &mut at, form, index as u8, &tail, immediate);
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2, 378);
    let mut aliases = Vec::new();
    for register in 0_u8..8 {
        aliases.push((
            register,
            match register {
                4 => vec![4, 0x24],
                5 => vec![0x45, 0],
                _ => vec![register],
            },
        ));
        if register != 4 {
            let mut tail = vec![4, (register << 3) | 5];
            tail.extend(0x4180_u32.to_le_bytes());
            aliases.push((register, tail));
            let mut tail = vec![
                if register == 5 { 0x44 } else { 4 },
                (register << 3) | register,
            ];
            if register == 5 {
                tail.push(0);
            }
            aliases.push((register, tail));
        }
    }
    assert_eq!(aliases.len(), 22);
    let alias_forms: Vec<_> = [0, 1, 4]
        .into_iter()
        .flat_map(|form| {
            aliases
                .iter()
                .map(move |(register, tail)| (form, *register, tail))
        })
        .collect();
    let mut alias_blocks = Vec::new();
    for (half, rows) in alias_forms.chunks_exact(33).enumerate() {
        let start = 0x200 + half * 0x200;
        at = start;
        for &(form, register, tail) in rows {
            append_memory_predicate(&mut bank, &mut at, form, register, tail, 1);
        }
        bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
        alias_blocks.push((0x1000 + start as u32, at + 2 - start));
    }
    at = 0x600;
    for form in [0, 1, 4] {
        for (register, tail) in [
            (3, &[0x83, 0x80, 0x51, 0, 0][..]),
            (6, &[4, 0xf5, 0x80, 0x41, 0, 0]),
            (1, &[0x84, 0x0b, 0x80, 0x41, 0, 0]),
            (0, &[0x84, 0x30, 0x80, 0x41, 0, 0]),
            (5, &[0x84, 0x3d, 0x80, 0x41, 0, 0]),
            (2, &[4, 0x95, 0x80, 0x41, 0, 0]),
            (3, &[0x43, 0xff]),
            (4, &[0x44, 0x24, 0xff]),
        ] {
            append_memory_predicate(&mut bank, &mut at, form, register, tail, 1);
        }
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x600, 170);
    at = 0x800;
    for form in 0..6 {
        bank[at] = 0x90;
        at += 1;
        append_memory_predicate(
            &mut bank,
            &mut at,
            form,
            0,
            &[3],
            if form == 3 { 0xff } else { 1 },
        );
    }
    bank[at..at + 2].copy_from_slice(&[0xeb, 0xfe]);
    assert_eq!(at + 2 - 0x800, 31);
    let mut control = vec![(0x1800, 31)];
    for (path, raw) in [
        "663903660f40d1660f41ef0f41f50f90c40f91c17002",
        "663b03660f42d1660f43ef0f43f50f92c40f93c17202",
        "668503660f48d1660f49ef0f49f50f98c40f99c17802",
    ]
    .into_iter()
    .enumerate()
    {
        let bytes: Vec<u8> = (0..raw.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&raw[index..index + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 22);
        let offset = 0xa00 + path * 64;
        bank[offset..offset + 22].copy_from_slice(&bytes);
        bank[offset + 22..offset + 26].copy_from_slice(&[0x0f, 0x0b, 0xeb, 0]);
        control.extend([
            (0x1000 + offset as u32, 22),
            (0x1000 + offset as u32 + 24, 2),
        ]);
    }
    bank[0xf00..0xf02].copy_from_slice(&[0x0f, 0x0b]);
    let groups = vec![
        PredicateInputGroup {
            id: "main",
            blocks: vec![(0x1000, 378)],
            instructions: 49,
        },
        PredicateInputGroup {
            id: "alias-a",
            blocks: vec![alias_blocks[0]],
            instructions: 34,
        },
        PredicateInputGroup {
            id: "alias-b",
            blocks: vec![alias_blocks[1]],
            instructions: 34,
        },
        PredicateInputGroup {
            id: "wrap",
            blocks: vec![(0x1600, 170)],
            instructions: 25,
        },
        PredicateInputGroup {
            id: "control",
            blocks: control,
            instructions: 37,
        },
        PredicateInputGroup {
            id: "helper",
            blocks: vec![(0x1800, 4)],
            instructions: 2,
        },
        PredicateInputGroup {
            id: "keeper",
            blocks: vec![(0x1801, 3)],
            instructions: 1,
        },
    ];
    (bank, groups)
}

#[test]
fn capture_word_memory_predicate_raw_inputs_without_guest_execution() {
    let directory = std::env::var_os("RING3_WORD_MEMORY_PREDICATE_INPUT_DIR").map(PathBuf::from);
    if let Some(directory) = &directory {
        assert!(directory.is_absolute());
        let metadata = fs::symlink_metadata(directory).unwrap();
        assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
        assert_eq!(fs::canonicalize(directory).unwrap(), *directory);
    }
    let (bank, groups) = memory_predicate_input_bank();
    let mut data: Vec<u8> = (0..4096)
        .map(|index| (index as u8).wrapping_mul(73).wrapping_add(11))
        .collect();
    for form in 0_u8..6 {
        for (index, (left, right)) in predicate_input_pairs(form).into_iter().enumerate() {
            let value = if form == 1 { right } else { left };
            let at = 2 * (usize::from(form) * 8 + index);
            data[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    for (at, value) in [(0x100, 0x8000_u16), (0x102, 1), (0x180, 0x8001)] {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
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
