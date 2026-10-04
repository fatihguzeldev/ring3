use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Location32, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

#[test]
fn memory_imul_two_operand_admits_with_unmapped_data() {
    let bytes = [0x0f, 0xaf, 0x03, 0xeb, 0x00];
    let mut results = Vec::new();
    for resident in [false, true] {
        for entries in [false, true] {
            let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
            engine.map(0x1000, 1, 7).unwrap();
            engine.arena_mut().unwrap()[140..145].copy_from_slice(&bytes);
            engine.upload(0x1000, bytes.len() as u32).unwrap();
            engine.protect(0x1000, 1, 4).unwrap();
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(0), Access::Read)
                    .is_err()
            );
            let transfer = &mut engine.arena_mut().unwrap()[140..];
            transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
            if !entries {
                transfer[4..8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            }
            let result = match (resident, entries) {
                (false, false) => engine.compile(1).map(u64::from),
                (false, true) => engine.compile_entries(1, 0).map(u64::from),
                (true, false) => engine.compile_resident(1).map(|id| id.get()),
                (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
            };
            results.push((resident, entries, result));
        }
    }
    assert!(
        results.iter().all(|(_, _, result)| result.is_ok()),
        "bound memory IMUL admission results: {results:?}"
    );
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const FORMS: [&[u8]; 5] = [
    &[0x0f, 0xaf, 0x05, 0x10, 0x50, 0, 0],
    &[0x69, 0x05, 0x10, 0x50, 0, 0, 0x78, 0x56, 0x34, 0x92],
    &[0x6b, 0x05, 0x10, 0x50, 0, 0, 0x80],
    &[0x69, 0x05, 0x10, 0x50, 0, 0, 0, 0, 0, 0],
    &[0x6b, 0x05, 0x10, 0x50, 0, 0, 0],
];

#[derive(Clone, Copy, Debug)]
enum Owner {
    Replacement,
    Resident,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn code(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    let stride = if entries { 4 } else { 8 };
    for (index, (pc, length)) in specs.iter().enumerate() {
        let offset = index * stride;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&(*length as u32).to_le_bytes());
        }
    }
}

fn compile(
    engine: &mut EngineInstance,
    owner: Owner,
    entries: bool,
    count: u32,
) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(count).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(count, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(count).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    }
}

fn module(engine: &EngineInstance, owner: Owner, id: u64) -> &[u8] {
    match owner {
        Owner::Replacement => engine.artifact_bytes().unwrap(),
        Owner::Resident => engine.resident_bytes(id).unwrap(),
    }
}

fn guard(engine: &EngineInstance, owner: Owner, id: u64) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => engine.guard(KEY, id as u32),
        Owner::Resident => engine.guard_resident(KEY, id),
    }
}

fn compile_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn u32_leb(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let (&byte, remaining) = bytes.split_first().unwrap();
        *bytes = remaining;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
    }
    panic!("invalid unsigned LEB128");
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = u32_leb(bytes) as usize;
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    std::str::from_utf8(value).unwrap()
}

fn imports(wasm: &[u8]) -> Vec<(String, String)> {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut bytes = &wasm[8..];
    while !bytes.is_empty() {
        let (&kind, remaining) = bytes.split_first().unwrap();
        bytes = remaining;
        let length = u32_leb(&mut bytes) as usize;
        let (mut section, remaining) = bytes.split_at(length);
        bytes = remaining;
        if kind != 2 {
            continue;
        }
        let count = u32_leb(&mut section);
        let mut names = Vec::new();
        for _ in 0..count {
            names.push((name(&mut section).to_owned(), name(&mut section).to_owned()));
            let (&kind, remaining) = section.split_first().unwrap();
            section = remaining;
            match kind {
                0 => {
                    u32_leb(&mut section);
                }
                2 => {
                    assert_eq!(u32_leb(&mut section), 0);
                    assert_eq!(u32_leb(&mut section), 1);
                }
                _ => panic!("unexpected import kind {kind}"),
            }
        }
        assert!(section.is_empty());
        return names;
    }
    panic!("missing Wasm import section");
}

fn assert_read_only_imports(engine: &EngineInstance, owner: Owner, id: u64) {
    let guard = match owner {
        Owner::Replacement => "guard",
        Owner::Resident => "guard_resident",
    };
    assert_eq!(
        imports(module(engine, owner, id)),
        vec![
            ("env".to_owned(), "memory".to_owned()),
            ("ring3".to_owned(), guard.to_owned()),
            ("ring3".to_owned(), "read32".to_owned()),
        ]
    );
}

#[test]
fn three_memory_forms_and_both_zero_immediates_admit_without_reading_data() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0, 0x0f, 0x0b]);
        let length = instruction.len() + 2;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), length)
                    .unwrap();
                engine.arena_mut().unwrap().fill(0xa5);
                describe(&mut engine, &[(CODE, length)], entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(CODE), Access::Read)
                        .is_err()
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA + 0x10), Access::Read)
                        .is_err()
                );
                assert_read_only_imports(&engine, owner, id);
                guard(&engine, owner, id).unwrap();
                match owner {
                    Owner::Replacement => assert_eq!(engine.generation(), id as u32),
                    Owner::Resident => {
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.lookup_resident(CODE).unwrap().get(), id);
                        assert_eq!(
                            engine
                                .lookup_resident(CODE + instruction.len() as u32)
                                .unwrap()
                                .get(),
                            id
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn original_base_index_esp_and_no_base_addresses_remain_typed_and_bound() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    let cases: &[(&[u8], Register32, EffectiveAddress, Option<u32>)] = &[
        (
            &[0x0f, 0xaf, 0x04, 0x48],
            Eax,
            EffectiveAddress {
                base: Some(Eax),
                index: Some(Ecx),
                scale: 2,
                displacement: 0,
            },
            None,
        ),
        (
            &[0x69, 0x4c, 0x8b, 0xfe, 0x78, 0x56, 0x34, 0x92],
            Ecx,
            EffectiveAddress {
                base: Some(Ebx),
                index: Some(Ecx),
                scale: 4,
                displacement: 0xffff_fffe,
            },
            Some(0x9234_5678),
        ),
        (
            &[0x6b, 0x64, 0x24, 0x08, 0x80],
            Esp,
            EffectiveAddress {
                base: Some(Esp),
                index: None,
                scale: 1,
                displacement: 8,
            },
            Some(0xffff_ff80),
        ),
        (
            &[0x69, 0x3f, 0, 0, 0, 0],
            Edi,
            EffectiveAddress {
                base: Some(Edi),
                index: None,
                scale: 1,
                displacement: 0,
            },
            Some(0),
        ),
        (
            &[0x69, 0x14, 0x8d, 0xf8, 0xff, 0xff, 0xff, 0, 0, 1, 0],
            Edx,
            EffectiveAddress {
                base: None,
                index: Some(Ecx),
                scale: 4,
                displacement: 0xffff_fff8,
            },
            Some(0x0001_0000),
        ),
        (
            &[0x6b, 0x75, 0xf8, 0xff],
            Esi,
            EffectiveAddress {
                base: Some(Ebp),
                index: None,
                scale: 1,
                displacement: 0xffff_fff8,
            },
            Some(u32::MAX),
        ),
        (
            &[0x0f, 0xaf, 0x35, 0xfc, 0xff, 0xff, 0xff],
            Esi,
            EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: 0xffff_fffc,
            },
            None,
        ),
    ];
    for &(instruction, destination, address, immediate) in cases {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.length() as usize, instruction.len());
        assert_eq!(
            decoded.next_pc(),
            GuestAddress(CODE + instruction.len() as u32)
        );
        assert_eq!(
            decoded.operation(),
            &Operation::SignedMultiply {
                destination,
                source: Location32::Memory(address),
                immediate,
            }
        );
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_read_only_imports(&engine, owner, id);
            }
        }
    }
}

#[test]
fn exact_terminal_fetch_needs_no_successor_code_or_data_page() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let pc = 0x2000 - bytes.len() as u32;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(pc, &bytes, true);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(pc), bytes.len())
                    .unwrap();
                describe(&mut engine, &[(pc, bytes.len())], entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                assert_read_only_imports(&engine, owner, id);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA + 0x10), Access::Read)
                        .is_err()
                );
            }
        }
    }
}

#[test]
fn missing_last_operand_byte_reports_execute_fault_before_publication() {
    for instruction in FORMS {
        let missing = &instruction[..instruction.len() - 1];
        let pc = 0x2000 - missing.len() as u32;
        let error = instruction_error(
            pc,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped,
                },
                length: instruction.len() as u32,
            }),
        );
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(pc, missing, true);
                describe(&mut engine, &[(pc, instruction.len())], entries);
                let before = engine.arena().to_vec();
                let arena_pointer = engine.arena_address();
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, error))
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.arena_address(), arena_pointer);
                assert_eq!(engine.generation(), 0);
                assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.lookup_resident(pc).is_err());
            }
        }
    }
}

#[test]
fn seeds_and_explicit_extents_cannot_split_a_memory_operand() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                let count = if entries {
                    describe(
                        &mut engine,
                        &[(CODE, 0), (CODE + instruction.len() as u32 - 1, 0)],
                        true,
                    );
                    2
                } else {
                    describe(&mut engine, &[(CODE, instruction.len() - 1)], false);
                    1
                };
                let expected = if entries {
                    CompileError::InvalidBlocks
                } else {
                    instruction_error(CODE, InstructionError::InvalidBlockEnd)
                };
                let before = engine.arena().to_vec();
                assert_eq!(
                    compile(&mut engine, owner, entries, count),
                    Err(compile_error(owner, expected))
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), 0);
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn adjacent_memory_seeds_preserve_both_consumed_instruction_entries() {
    let bytes = [
        0x0f, 0xaf, 0x03, 0x69, 0x3f, 0, 0, 0, 0, 0xeb, 0, 0x0f, 0x0b,
    ];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, &[(CODE, 3), (CODE + 3, 8)], entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries, 2).unwrap();
            assert_eq!(engine.arena(), before);
            assert_read_only_imports(&engine, owner, id);
            if matches!(owner, Owner::Resident) {
                for pc in [CODE, CODE + 3, CODE + 9] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 11).is_err());
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    replacement: Vec<u8>,
    replacement_pointer: usize,
    resident: Vec<u8>,
    resident_pointer: usize,
    mapped_pages: u32,
}

fn saved(engine: &EngineInstance, id: u64) -> Saved {
    let replacement = engine.artifact_bytes().unwrap();
    let resident = engine.resident_bytes(id).unwrap();
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        replacement: replacement.to_vec(),
        replacement_pointer: replacement.as_ptr() as usize,
        resident: resident.to_vec(),
        resident_pointer: resident.as_ptr() as usize,
        mapped_pages: engine.memory().unwrap().mapped_pages(),
    }
}

fn prior_owners(bytes: &[u8]) -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, bytes);
    describe(&mut engine, &[(KEEP, bytes.len())], false);
    engine.compile(1).unwrap();
    describe(&mut engine, &[(KEEP, bytes.len())], false);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

#[test]
fn public_default_limit_accepts_sixty_four_and_precedes_later_poison() {
    for instruction in FORMS {
        let mut valid = instruction.repeat(63);
        valid.extend([0xeb, 0]);
        let mut invalid = instruction.repeat(64);
        invalid.extend([0x0f, 0x0b]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = EngineInstance::new(2, KEY).unwrap();
                engine.map(CODE, 1, 7).unwrap();
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &valid);
                describe(&mut engine, &[(KEEP, valid.len())], entries);
                let admitted = compile(&mut engine, owner, entries, 1).unwrap();
                assert_read_only_imports(&engine, owner, admitted);
                describe(&mut engine, &[(KEEP, valid.len())], false);
                let keep = match owner {
                    Owner::Replacement => engine.compile_resident(1).unwrap().get(),
                    Owner::Resident => {
                        engine.compile(1).unwrap();
                        admitted
                    }
                };
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(KEEP), valid.len())
                    .unwrap();
                upload(&mut engine, CODE, &invalid);
                describe(&mut engine, &[(CODE, invalid.len())], entries);
                let before = saved(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, CompileError::InstructionLimit))
                );
                assert_eq!(saved(&engine, keep), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}

#[test]
fn data_changes_preserve_code_but_consumed_operand_bytes_invalidate_owner() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let pc = 0x1ffe;
        for offset in [2, instruction.len() - 1] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes, false);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(pc), bytes.len())
                        .unwrap();
                    describe(&mut engine, &[(pc, bytes.len())], entries);
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    let preserved = module(&engine, owner, id).to_vec();
                    let pointer = module(&engine, owner, id).as_ptr();
                    engine.map(DATA, 1, 3).unwrap();
                    upload(&mut engine, DATA + 0x10, &[0xfd, 0xff, 0xff, 0xff]);
                    engine.protect(DATA, 1, 1).unwrap();
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    assert_eq!(module(&engine, owner, id), preserved);
                    assert_eq!(module(&engine, owner, id).as_ptr(), pointer);
                    guard(&engine, owner, id).unwrap();
                    upload(&mut engine, pc + offset as u32, &[instruction[offset] ^ 1]);
                    assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                    let expected = match owner {
                        Owner::Replacement => HostError::CodeInvalidated,
                        Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
                    };
                    assert_eq!(guard(&engine, owner, id), Err(expected));
                    match owner {
                        Owner::Replacement => assert_eq!(engine.artifact_bytes(), Err(expected)),
                        Owner::Resident => assert_eq!(engine.resident_bytes(id), Err(expected)),
                    }
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 3);
                }
            }
        }
    }
}

#[test]
fn admitted_memory_prefix_then_ud2_preserves_prior_owners_and_publication() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners(&[0x90, 0xeb, 0]);
            let keep_snapshot = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(KEEP), 3)
                .unwrap();
            for instruction in FORMS {
                let mut bytes = vec![0x0f, 0xaf, 0x03];
                bytes.extend_from_slice(instruction);
                bytes.extend([0x0f, 0x0b]);
                upload(&mut engine, CODE, &bytes);
                let candidate_snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = saved(&engine, keep);
                let error = instruction_error(
                    CODE + 3 + instruction.len() as u32,
                    InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                );
                assert_eq!(
                    compile(&mut engine, owner, entries, 1),
                    Err(compile_error(owner, error))
                );
                assert_eq!(saved(&engine, keep), before);
                assert!(engine.memory().unwrap().is_code_current(&keep_snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(&candidate_snapshot)
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA + 0x10), Access::Read)
                        .is_err()
                );
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}
