use super::support_byte_prefix_admission as byte_prefix_admission;

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn admits_byte_carry_rotate_cl(bytes: &[u8; 4]) {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 4,
        }],
        CompileLimits::default(),
    )
    .expect("register byte carry rotate by old CL must compile");
    assert_eq!(compiled.metadata().blocks, 1);
    assert_eq!(compiled.metadata().instructions, 2);
}

#[test]
fn register_byte_rcl_by_cl_admits_before_jump() {
    admits_byte_carry_rotate_cl(&[0xd2, 0xd0, 0xeb, 0]);
}

#[test]
fn register_byte_rcr_by_cl_admits_before_jump() {
    admits_byte_carry_rotate_cl(&[0xd2, 0xd8, 0xeb, 0]);
}

use std::collections::BTreeSet;

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            ArtifactError, CompileError, InstructionError, RegistryError, compile_entry_region,
            prepare_entry_region, prepare_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, Operation, RotateKind},
        },
    },
    memory::{Access, FaultReason, MemoryFault},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x1234_5678_9abc_def0;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KINDS: [RotateKind; 2] = [RotateKind::Left, RotateKind::Right];
const BYTES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn instruction(opcode: u8, kind: RotateKind, alias: u8, raw: u8) -> Vec<u8> {
    let mut bytes = vec![
        opcode,
        0xd0 | u8::from(kind == RotateKind::Right) << 3 | alias,
    ];
    if opcode == 0xc0 {
        bytes.push(raw);
    }
    bytes
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
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

fn spec(pc: u32, length: usize) -> BlockSpec {
    BlockSpec {
        entry: GuestAddress(pc),
        byte_length: length as u32,
    }
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..].copy_from_slice(&if entries { 0 } else { length as u32 }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, resident: bool, entries: bool) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => engine.compile(1).map(u64::from),
        (false, true) => engine.compile_entries(1, 0).map(u64::from),
        (true, false) => engine.compile_resident(1).map(|id| id.get()),
        (true, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn page(memory: &AddressSpace, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.fetch(GuestAddress(pc), &mut bytes).unwrap();
    bytes
}

fn error(pc: u32, cause: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(cause),
    }
}

fn fetch_error(pc: u32, address: u32, length: u32, reason: FaultReason) -> DecodeError {
    DecodeError::MemoryFault {
        pc: GuestAddress(pc),
        fault: MemoryFault {
            address: GuestAddress(address),
            access: Access::Execute,
            reason,
        },
        length,
    }
}

fn validate_modules(modules: &[(u8, Vec<u8>)]) {
    for (owner, bytes) in modules {
        assert!(*owner <= 2);
        assert!(bytes.len() <= CompileLimits::default().wasm_bytes);
        if *owner == 0 {
            crate::support_wasm_interface::assert_interface(bytes, &[], &[4], false);
        } else {
            let resident = *owner == 2;
            crate::support_wasm_interface::assert_bound(
                bytes,
                resident,
                &[],
                &[4, if resident { 7 } else { 6 }],
                false,
            );
        }
    }
}

#[test]
fn all_byte_aliases_keep_carry_cl_and_old_one_decode_identities_distinct() {
    let mut engine = code(CODE, &[0x90; 3], false);
    let mut identities = BTreeSet::new();
    let (mut cl, mut implicit, mut immediate) = (0, 0, 0);
    for kind in KINDS {
        for (alias, destination) in BYTES.into_iter().enumerate() {
            for opcode in [0xd2, 0xd0, 0xc0] {
                let raws: &[u8] = if opcode == 0xc0 {
                    &[1, 33, 65, 97, 129, 161, 193, 225]
                } else {
                    &[0]
                };
                for &raw in raws {
                    let bytes = instruction(opcode, kind, alias as u8, raw);
                    assert!(identities.insert(bytes.clone()));
                    upload(&mut engine, CODE, &bytes);
                    let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                    let expected = if opcode == 0xd2 {
                        cl += 1;
                        Operation::ByteRotateThroughCarryCl { kind, destination }
                    } else {
                        if opcode == 0xd0 {
                            implicit += 1;
                        } else {
                            immediate += 1;
                        }
                        Operation::ByteRotateThroughCarryOne { kind, destination }
                    };
                    assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
                    assert_eq!(
                        (decoded.length(), decoded.next_pc()),
                        (bytes.len() as u8, GuestAddress(CODE + bytes.len() as u32))
                    );
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .is_code_current(decoded.code_snapshot())
                    );
                }
            }
        }
    }
    assert_eq!(
        (cl, implicit, immediate, identities.len()),
        (16, 16, 128, 160)
    );
}

#[test]
fn all_six_profiles_validate_guard_only_imports_types_and_fixed_locals() {
    let mut modules = Vec::new();
    for kind in KINDS {
        for alias in 0..8 {
            let mut bytes = instruction(0xd2, kind, alias, 0);
            bytes.extend([0xeb, 0]);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            let before = page(memory, CODE);
            let artifacts = [
                compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default())
                    .unwrap(),
                compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
                    .unwrap(),
            ];
            assert_eq!(artifacts[0].metadata(), artifacts[1].metadata());
            assert_eq!(
                artifacts[0].wasm_bytes(memory).unwrap(),
                artifacts[1].wasm_bytes(memory).unwrap()
            );
            for artifact in artifacts {
                let m = artifact.metadata();
                assert_eq!(
                    (
                        m.backend_version,
                        m.abi_version,
                        m.profile,
                        m.blocks,
                        m.instructions
                    ),
                    (1, 1, 1, 1, 2)
                );
                modules.push((0, artifact.wasm_bytes(memory).unwrap().to_vec()));
            }
            assert_eq!(page(memory, CODE), before);
            for resident in [false, true] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes, true);
                    describe(&mut engine, CODE, bytes.len(), entries);
                    let before = engine.arena().to_vec();
                    let id = compile(&mut engine, resident, entries).unwrap();
                    assert_eq!(engine.arena(), before);
                    assert_eq!(page(engine.memory().unwrap(), CODE), page(memory, CODE));
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
                            .resolve(GuestAddress(DATA), Access::Read)
                            .is_err()
                    );
                    if resident {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        for offset in 0..=bytes.len() {
                            let found = engine.lookup_resident(CODE + offset as u32);
                            if [0, 2].contains(&offset) {
                                assert_eq!(found.unwrap().get(), id);
                            } else {
                                assert!(found.is_err());
                            }
                        }
                        modules.push((2, engine.resident_bytes(id).unwrap().to_vec()));
                    } else {
                        engine.guard(KEY, id as u32).unwrap();
                        assert_eq!(engine.generation(), id as u32);
                        modules.push((1, engine.artifact_bytes().unwrap().to_vec()));
                    }
                }
            }
        }
    }
    assert_eq!(modules.len(), 96);
    validate_modules(&modules);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let unsupported = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = Vec::new();
    for kind in KINDS {
        for alias in 0..8 {
            let bytes = instruction(0xd2, kind, alias, 0);
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let expected = match prefix {
                    0xf0 => DecodeError::InvalidEncoding,
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    _ => unsupported,
                };
                cases.push(([vec![prefix], bytes.clone()].concat(), expected));
            }
        }
        for alias in [0, 5] {
            for raw in [0, 2, 8, 9, 31, 32, 255] {
                cases.push((
                    [vec![0x66], instruction(0xc0, kind, alias, raw)].concat(),
                    unsupported,
                ));
            }
            cases.push((
                vec![
                    0x66,
                    0x66,
                    0xd3,
                    0xd0 | u8::from(kind == RotateKind::Right) << 3 | alias,
                ],
                unsupported,
            ));
        }
    }
    for field in [2, 3] {
        for alias in 0..8 {
            let mut bytes = vec![0xd2, field << 3 | alias];
            if alias == 4 {
                bytes.push(0x24);
            } else if alias == 5 {
                bytes.extend([0; 4]);
            }
            bytes.insert(0, 0x66);
            cases.push((bytes, unsupported));
        }
    }
    assert_eq!(cases.len(), 224);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        224
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == DecodeError::InvalidEncoding)
            .count(),
        16
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        96
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, error)| *error == unsupported)
            .count(),
        112
    );
    cases
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
    pages: [Vec<u8>; 2],
}

fn saved(engine: &EngineInstance, resident: u64) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP].map(|pc| page(engine.memory().unwrap(), pc)),
    }
}

#[test]
fn closed_profiles_and_late_failures_preserve_both_prior_publications() {
    for (excluded, expected) in exclusions() {
        let (excluded, expected) = byte_prefix_admission::closed_fixture(&excluded, expected);
        let mut bytes = instruction(0xd2, RotateKind::Left, 1, 0);
        bytes.extend_from_slice(&excluded);
        let failure = error(CODE + 2, expected);
        let engine = code(CODE, &bytes, false);
        let memory = engine.memory().unwrap();
        let before = page(memory, CODE);
        crate::support_fetch::assert_decode_contract(
            (decode_one(memory, GuestAddress(CODE + 2)))
                .expect_err("instruction must fail to decode"),
            expected,
        );
        crate::support_fetch::assert_compile_contract(
            compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default())
                .unwrap_err(),
            failure,
        );
        crate::support_fetch::assert_compile_contract(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default())
                .unwrap_err(),
            failure,
        );
        assert_eq!(page(memory, CODE), before);
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, false);
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                describe(&mut engine, KEEP, 3, false);
                engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                describe(&mut engine, CODE, bytes.len(), entries);
                if entries
                    && crate::support_admission::admit_if_deferred(
                        &mut engine,
                        crate::support_admission::ColdEntryCase {
                            key: KEY,
                            keeper: keep,
                            resident,
                            pages: &[CODE, KEEP],
                            expected: failure,
                        },
                    )
                {
                    continue;
                }
                let before = saved(&engine, keep);
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, resident, entries))
                        .expect_err("compilation must refuse this fixture"),
                    expected,
                );
                assert_eq!(saved(&engine, keep), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
    for truncated in [true, false] {
        let (pc, bytes, length, failure) = if truncated {
            (
                0x1ffd,
                vec![0xd2, 0xd9, 0xd2],
                4,
                error(
                    0x1fff,
                    fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
                ),
            )
        } else {
            let mut bytes = vec![0x90; 63];
            bytes.extend([0xd2, 0xd9, 0xf0, 0x90]);
            let length = bytes.len();
            (CODE, bytes, length, CompileError::InstructionLimit)
        };
        let engine = code(pc, &bytes, false);
        let memory = engine.memory().unwrap();
        let before = page(memory, CODE);
        crate::support_fetch::assert_compile_contract(
            compile_region(memory, &[spec(pc, length)], CompileLimits::default()).unwrap_err(),
            failure,
        );
        let cold = compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default());
        if failure == CompileError::InstructionLimit {
            assert_eq!(cold.unwrap().metadata().instructions, 64);
        } else {
            crate::support_fetch::assert_compile_contract(cold.unwrap_err(), failure);
        }
        assert_eq!(page(memory, CODE), before);
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(pc, &bytes, false);
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                describe(&mut engine, KEEP, 3, false);
                engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                describe(&mut engine, pc, length, entries);
                if entries
                    && crate::support_admission::admit_if_deferred(
                        &mut engine,
                        crate::support_admission::ColdEntryCase {
                            key: KEY,
                            keeper: keep,
                            resident,
                            pages: &[CODE, KEEP],
                            expected: failure,
                        },
                    )
                {
                    continue;
                }
                let before = saved(&engine, keep);
                let expected = if resident {
                    HostError::Resident(RegistryError::Compile(failure))
                } else {
                    HostError::Compile(failure)
                };
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, resident, entries))
                        .expect_err("compilation must refuse this fixture"),
                    expected,
                );
                assert_eq!(saved(&engine, keep), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(pc).is_err());
            }
        }
    }
}

#[test]
fn two_byte_fetch_cuts_page_edges_and_top_wrap_keep_exact_faults() {
    let mut positives = 0;
    let mut cuts = 0;
    for kind in KINDS {
        for alias in 0..8 {
            let bytes = instruction(0xd2, kind, alias, 0);
            for pc in [0x1ffe, 0xffff_fffe] {
                let engine = code(pc, &bytes, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::ByteRotateThroughCarryCl {
                        kind,
                        destination: BYTES[alias as usize]
                    }
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (2, GuestAddress(pc.wrapping_add(2)))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc == 0x1ffe {
                    assert!(
                        memory
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                } else {
                    for artifact in [
                        compile_region(memory, &[spec(pc, 2)], CompileLimits::default()).unwrap(),
                        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                            .unwrap(),
                    ] {
                        assert_eq!(artifact.metadata().instructions, 1);
                    }
                }
                positives += 1;
            }
            for (pc, address, reason) in [
                (0x1fff, 0x2000, FaultReason::Unmapped),
                (u32::MAX, u32::MAX, FaultReason::AddressOverflow),
            ] {
                let engine = code(pc, &bytes[..1], true);
                crate::support_fetch::assert_decode_contract(
                    (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                        .expect_err("instruction must fail to decode"),
                    fetch_error(pc, address, 2, reason),
                );
                cuts += 1;
            }
            let mut engine = code(0x1fff, &bytes, false);
            engine.protect(0x2000, 1, 1).unwrap();
            crate::support_fetch::assert_decode_contract(
                (decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)))
                    .expect_err("instruction must fail to decode"),
                fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission),
            );
            let mut engine = code(CODE, &bytes, false);
            engine.protect(CODE, 1, 1).unwrap();
            crate::support_fetch::assert_decode_contract(
                (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                    .expect_err("instruction must fail to decode"),
                fetch_error(CODE, CODE, 1, FaultReason::Permission),
            );
            let engine = code(CODE, &bytes, true);
            crate::support_fetch::assert_compile_contract(
                prepare_region(
                    engine.memory().unwrap(),
                    &[spec(CODE, 1)],
                    CompileLimits::default(),
                )
                .unwrap_err(),
                CompileError::Instruction {
                    pc: GuestAddress(CODE),
                    cause: InstructionError::InvalidBlockEnd,
                },
            );
        }
    }
    assert_eq!((positives, cuts), (32, 32));
}

#[test]
fn consumed_opcode_and_alias_currency_are_distinct_from_unconsumed_tail() {
    for kind in KINDS {
        for alias in [0, 1, 5] {
            let bytes = instruction(0xd2, kind, alias, 0);
            for changed in 0..2 {
                for same in [false, true] {
                    for entries in [false, true] {
                        let pc = 0x1fff;
                        let mut block = bytes.clone();
                        block.extend([0xeb, 0]);
                        let mut engine = code(pc, &block, false);
                        let memory = engine.memory().unwrap();
                        let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                        let prepared = [
                            prepare_region(
                                memory,
                                &[spec(pc, block.len())],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                            prepare_entry_region(
                                memory,
                                &[GuestAddress(pc)],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                        ];
                        let artifacts = [
                            compile_region(
                                memory,
                                &[spec(pc, block.len())],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                            compile_entry_region(
                                memory,
                                &[GuestAddress(pc)],
                                CompileLimits::default(),
                            )
                            .unwrap(),
                        ];
                        describe(&mut engine, pc, block.len(), entries);
                        let generation = compile(&mut engine, false, entries).unwrap() as u32;
                        describe(&mut engine, pc, block.len(), entries);
                        let resident = compile(&mut engine, true, entries).unwrap();
                        engine.map(DATA, 1, 3).unwrap();
                        upload(&mut engine, DATA, &[0xa5]);
                        for prepared in &prepared {
                            assert!(prepared.is_current(engine.memory().unwrap()));
                        }
                        for artifact in &artifacts {
                            artifact.wasm_bytes(engine.memory().unwrap()).unwrap();
                        }
                        engine.guard(KEY, generation).unwrap();
                        engine.guard_resident(KEY, resident).unwrap();
                        let replacement = if same {
                            bytes[changed]
                        } else if changed == 0 {
                            0xd0
                        } else {
                            bytes[1] ^ 8
                        };
                        assert_eq!(replacement == bytes[changed], same);
                        upload(&mut engine, pc + changed as u32, &[replacement]);
                        assert!(
                            !engine
                                .memory()
                                .unwrap()
                                .is_code_current(decoded.code_snapshot())
                        );
                        for prepared in &prepared {
                            assert!(!prepared.is_current(engine.memory().unwrap()));
                        }
                        for artifact in &artifacts {
                            assert_eq!(
                                artifact.wasm_bytes(engine.memory().unwrap()),
                                Err(ArtifactError::CodeInvalidated)
                            );
                        }
                        let before = engine.arena().to_vec();
                        assert_eq!(
                            engine.guard(KEY, generation),
                            Err(HostError::CodeInvalidated)
                        );
                        assert_eq!(
                            engine.guard_resident(KEY, resident),
                            Err(HostError::Resident(RegistryError::CodeInvalidated))
                        );
                        assert_eq!(engine.arena(), before);
                    }
                }
            }
        }
    }
    for entries in [false, true] {
        let pc = 0x1ffc;
        let mut engine = code(pc, &[0xd2, 0xd0, 0xeb, 0, 0xf0, 0x90], false);
        let memory = engine.memory().unwrap();
        let artifacts = [
            compile_region(memory, &[spec(pc, 4)], CompileLimits::default()).unwrap(),
            compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).unwrap(),
        ];
        describe(&mut engine, pc, 4, entries);
        let generation = compile(&mut engine, false, entries).unwrap() as u32;
        describe(&mut engine, pc, 4, entries);
        let resident = compile(&mut engine, true, entries).unwrap();
        upload(&mut engine, 0x2000, &[0x90]);
        for artifact in &artifacts {
            artifact.wasm_bytes(engine.memory().unwrap()).unwrap();
        }
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
        engine.protect(0x1000, 1, 4).unwrap();
        for artifact in &artifacts {
            assert_eq!(
                artifact.wasm_bytes(engine.memory().unwrap()),
                Err(ArtifactError::CodeInvalidated)
            );
        }
        assert_eq!(
            engine.guard(KEY, generation),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.guard_resident(KEY, resident),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
}

#[test]
fn default_caps_and_observed_module_size_remain_closed() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for kind in KINDS {
        let atom = instruction(0xd2, kind, 1, 0);
        let mut bytes = vec![0x90; 62];
        bytes.extend_from_slice(&atom);
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        assert_eq!(
            prepare_region(memory, &[spec(CODE, bytes.len())], limits)
                .unwrap()
                .instruction_count(),
            64
        );
        assert_eq!(
            prepare_entry_region(memory, &[GuestAddress(CODE)], limits)
                .unwrap()
                .instruction_count(),
            64
        );
        for artifact in [
            compile_region(memory, &[spec(CODE, bytes.len())], limits).unwrap(),
            compile_entry_region(memory, &[GuestAddress(CODE)], limits).unwrap(),
        ] {
            assert_eq!(artifact.metadata().instructions, 64);
        }
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                compile(&mut engine, resident, entries).unwrap();
                assert_eq!(engine.arena(), before);
            }
        }
        let mut bytes = vec![0x90; 63];
        bytes.extend_from_slice(&atom);
        bytes.extend([0xf0, 0x90]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        assert_eq!(
            prepare_region(memory, &[spec(CODE, bytes.len())], limits).err(),
            Some(CompileError::InstructionLimit)
        );
        assert_eq!(
            prepare_entry_region(memory, &[GuestAddress(CODE)], limits)
                .unwrap()
                .instruction_count(),
            (limits).instructions
        );
        for resident in [false, true] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, CODE, bytes.len(), entries);
                if entries {
                    crate::support_admission::assert_fresh_cold_limit(
                        &mut engine,
                        CODE,
                        bytes.len() as u32,
                        resident,
                    );
                    continue;
                }
                let before = engine.arena().to_vec();
                let failure = if resident {
                    HostError::Resident(RegistryError::Compile(CompileError::InstructionLimit))
                } else {
                    HostError::Compile(CompileError::InstructionLimit)
                };
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, resident, entries))
                        .expect_err("compilation must refuse this fixture"),
                    failure,
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.generation(), 0);
            }
        }
    }
    let bytes = [0xd2, 0xdd, 0xeb, 0];
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let actual = compile_region(memory, &[spec(CODE, bytes.len())], limits)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap()
        .len();
    for tight in [actual, actual - 1, 1] {
        let limits = CompileLimits {
            wasm_bytes: tight,
            ..limits
        };
        let explicit = compile_region(memory, &[spec(CODE, bytes.len())], limits);
        let entries = compile_entry_region(memory, &[GuestAddress(CODE)], limits);
        if tight == actual {
            assert!(explicit.is_ok());
        } else {
            assert_eq!(explicit.err(), Some(CompileError::WasmLimit));
        }
        crate::support_admission::assert_entry_byte_budget(
            memory,
            spec(CODE, bytes.len()),
            limits,
            entries,
        );
    }
    for invalid in [
        CompileLimits {
            blocks: 0,
            ..limits
        },
        CompileLimits {
            blocks: 9,
            ..limits
        },
        CompileLimits {
            instructions: 0,
            ..limits
        },
        CompileLimits {
            instructions: 65,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 0,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 65_537,
            ..limits
        },
    ] {
        crate::support_fetch::assert_compile_contract(
            compile_region(memory, &[spec(CODE, bytes.len())], invalid).unwrap_err(),
            CompileError::InvalidLimits,
        );
        crate::support_fetch::assert_compile_contract(
            compile_entry_region(memory, &[GuestAddress(CODE)], invalid).unwrap_err(),
            CompileError::InvalidLimits,
        );
    }
    let atom = [0xd2, 0xd0, 0xeb, 0];
    let bytes = atom.repeat(9);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let specs: Vec<_> = (0..9).map(|index| spec(CODE + index * 4, 4)).collect();
    let entries: Vec<_> = specs.iter().map(|spec| spec.entry).collect();
    for artifact in [
        compile_region(memory, &specs[..8], limits).unwrap(),
        compile_entry_region(memory, &entries[..8], limits).unwrap(),
    ] {
        assert_eq!(
            (artifact.metadata().blocks, artifact.metadata().instructions),
            (8, 16)
        );
    }
    crate::support_fetch::assert_compile_contract(
        prepare_region(memory, &specs, limits).unwrap_err(),
        CompileError::InvalidBlocks,
    );
    crate::support_fetch::assert_compile_contract(
        prepare_entry_region(memory, &entries, limits).unwrap_err(),
        CompileError::InvalidBlocks,
    );
}
