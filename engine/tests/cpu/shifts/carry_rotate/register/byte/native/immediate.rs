use super::support_byte_prefix_admission as byte_prefix_admission;

use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_region},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn admits_byte_carry_rotate_immediate(bytes: &[u8; 5]) {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    let compiled = compile_region(
        &memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 5,
        }],
        CompileLimits::default(),
    )
    .expect("register byte carry rotate by immediate must compile");
    assert_eq!(compiled.metadata().blocks, 1);
    assert_eq!(compiled.metadata().instructions, 2);
}

#[test]
fn register_byte_rcl_by_immediate_admits_before_jump() {
    admits_byte_carry_rotate_immediate(&[0xc0, 0xd0, 2, 0xeb, 0]);
}

#[test]
fn register_byte_rcr_by_immediate_admits_before_jump() {
    admits_byte_carry_rotate_immediate(&[0xc0, 0xd8, 2, 0xeb, 0]);
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
    process::{EngineInstance, HostError, ResidentInstallation},
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

fn expected(kind: RotateKind, destination: ByteRegister, raw: u8) -> Operation {
    if raw & 31 == 1 {
        Operation::ByteRotateThroughCarryOne { kind, destination }
    } else {
        Operation::ByteRotateThroughCarryImmediate {
            kind,
            destination,
            count: raw,
        }
    }
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
fn every_raw_immediate_and_alias_keeps_new_and_old_ir_identities() {
    let mut engine = code(CODE, &[0x90; 3], false);
    let mut identities = BTreeSet::new();
    let (mut old, mut new, mut implicit, mut cl) = (0, 0, 0, 0);
    for kind in KINDS {
        for (alias, destination) in BYTES.into_iter().enumerate() {
            for raw in 0..=u8::MAX {
                let bytes = instruction(0xc0, kind, alias as u8, raw);
                assert!(identities.insert(bytes.clone()));
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &expected(kind, destination, raw),
                    "{bytes:02x?}"
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (3, GuestAddress(CODE + 3))
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                if raw & 31 == 1 {
                    old += 1;
                } else {
                    new += 1;
                }
            }
            for opcode in [0xd0, 0xd2] {
                let bytes = instruction(opcode, kind, alias as u8, 0);
                assert!(identities.insert(bytes.clone()));
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                let operation = if opcode == 0xd0 {
                    implicit += 1;
                    Operation::ByteRotateThroughCarryOne { kind, destination }
                } else {
                    cl += 1;
                    Operation::ByteRotateThroughCarryCl { kind, destination }
                };
                assert_eq!(decoded.operation(), &operation);
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (2, GuestAddress(CODE + 2))
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
    assert_eq!(
        (old, new, implicit, cl, identities.len()),
        (128, 3968, 16, 16, 4128)
    );
}

#[test]
fn every_masked_count_and_selected_six_profiles_validate_fixed_wasm_shapes() {
    let mut modules = Vec::new();
    let mut targets = 0;
    for kind in KINDS {
        for q in 0..32 {
            let mut bytes = Vec::new();
            for alias in 0..8 {
                bytes.extend(instruction(0xc0, kind, alias, q));
                targets += 1;
            }
            bytes.extend([0xeb, 0]);
            let engine = code(CODE, &bytes, true);
            let memory = engine.memory().unwrap();
            let before = page(memory, CODE);
            let artifact =
                compile_region(memory, &[spec(CODE, bytes.len())], CompileLimits::default())
                    .unwrap();
            assert_eq!(
                (artifact.metadata().blocks, artifact.metadata().instructions),
                (1, 9)
            );
            modules.push((0, artifact.wasm_bytes(memory).unwrap().to_vec()));
            assert_eq!(page(memory, CODE), before);
        }
    }
    assert_eq!((modules.len(), targets), (64, 512));
    let mut profiles = 0;
    for kind in KINDS {
        for alias in 0..8 {
            let raw = [0, 2, 9, 10, 18, 19, 27, 255][alias as usize];
            let mut bytes = instruction(0xc0, kind, alias, raw);
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
                profiles += 1;
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
                            if [0, 3].contains(&offset) {
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
                    profiles += 1;
                }
            }
        }
    }
    assert_eq!((profiles, modules.len()), (96, 160));
    validate_modules(&modules);
}

fn exclusions() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut cases = Vec::new();
    for kind in KINDS {
        for alias in 0..8 {
            let bytes = instruction(0xc0, kind, alias, 2);
            for prefix in [
                0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
            ] {
                let cause = match prefix {
                    0xf0 => DecodeError::InvalidEncoding,
                    0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                        DecodeError::Unsupported(UnsupportedFeature::Segment)
                    }
                    _ => opcode,
                };
                cases.push(([vec![prefix], bytes.clone()].concat(), cause));
            }
        }
        for alias in [0, 5] {
            for opcode_byte in [0xc1, 0xd3] {
                let mut bytes = vec![0x66, 0x66];
                bytes.extend(instruction(opcode_byte, kind, alias, 2));
                if opcode_byte == 0xc1 {
                    bytes.push(2);
                }
                cases.push((bytes, opcode));
            }
        }
        for alias in 0..8 {
            let operand = 0x10 | u8::from(kind == RotateKind::Right) << 3 | alias;
            for opcode_byte in [0xc0, 0xd2] {
                let mut bytes = vec![opcode_byte, operand];
                if alias == 4 {
                    bytes.push(0x24);
                } else if alias == 5 {
                    bytes.extend([0; 4]);
                }
                if opcode_byte == 0xc0 {
                    bytes.push(2);
                }
                bytes.insert(0, 0x66);
                cases.push((bytes, opcode));
            }
        }
    }
    assert_eq!(cases.len(), 216);
    assert_eq!(
        cases
            .iter()
            .map(|(bytes, _)| bytes)
            .collect::<BTreeSet<_>>()
            .len(),
        216
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, cause)| *cause == DecodeError::InvalidEncoding)
            .count(),
        16
    );
    assert_eq!(
        cases
            .iter()
            .filter(|(_, cause)| *cause == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        96
    );
    assert_eq!(
        cases.iter().filter(|(_, cause)| *cause == opcode).count(),
        104
    );
    cases
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    authority: (bool, u64),
    entries: [u64; 2],
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 2],
    mapped: u32,
    installed: ResidentInstallation,
}

fn saved(engine: &EngineInstance, resident: u64) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        authority: (engine.is_open(), engine.key()),
        entries: [KEEP, KEEP + 1].map(|pc| engine.lookup_resident(pc).unwrap().get()),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP].map(|pc| page(engine.memory().unwrap(), pc)),
        mapped: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

fn retain_prior(engine: &mut EngineInstance, pc: u32, length: usize, failure: CompileError) {
    engine.map(KEEP, 1, 7).unwrap();
    upload(engine, KEEP, &[0x90, 0xeb, 0]);
    describe(engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let keep = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, keep, 3)
        .unwrap();
    for resident in [false, true] {
        for entries in [false, true] {
            describe(engine, pc, length, entries);
            if entries
                && crate::support_admission::admit_if_deferred(
                    engine,
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
            let before = saved(engine, keep);
            let expected = if resident {
                HostError::Resident(RegistryError::Compile(failure))
            } else {
                HostError::Compile(failure)
            };
            crate::support_fetch::assert_host_contract(
                (compile(engine, resident, entries))
                    .expect_err("compilation must refuse this fixture"),
                expected,
            );
            assert_eq!(saved(engine, keep), before);
            engine.guard(KEY, before.generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
            assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
            assert!(engine.lookup_resident(pc).is_err());
        }
    }
}

#[test]
fn closed_neighbors_and_late_failures_keep_both_published_owners() {
    for (excluded, expected) in exclusions() {
        let (excluded, expected) = byte_prefix_admission::closed_fixture(&excluded, expected);
        let mut bytes = instruction(0xc0, RotateKind::Left, 1, 2);
        bytes.extend_from_slice(&excluded);
        let failure = error(CODE + 3, expected);
        let mut engine = code(CODE, &bytes, false);
        let memory = engine.memory().unwrap();
        let before = page(memory, CODE);
        crate::support_fetch::assert_decode_contract(
            (decode_one(memory, GuestAddress(CODE + 3)))
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
        retain_prior(&mut engine, CODE, bytes.len(), failure);
    }
    for cut in [1, 2] {
        let pc = 0x2000 - 3 - cut as u32;
        let mut bytes = instruction(0xc0, RotateKind::Left, 1, 10);
        bytes.extend_from_slice(&[0xc0, 0xd8][..cut]);
        let fault_pc = pc + 3;
        let failure = error(
            fault_pc,
            fetch_error(fault_pc, 0x2000, cut as u32 + 1, FaultReason::Unmapped),
        );
        let mut engine = code(pc, &bytes, false);
        let memory = engine.memory().unwrap();
        crate::support_fetch::assert_compile_contract(
            compile_region(
                memory,
                &[spec(pc, bytes.len() + 1)],
                CompileLimits::default(),
            )
            .unwrap_err(),
            failure,
        );
        crate::support_fetch::assert_compile_contract(
            compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                .unwrap_err(),
            failure,
        );
        retain_prior(&mut engine, pc, bytes.len() + 1, failure);
    }
}

#[test]
fn all_operand_fetch_cuts_page_edges_and_top_wrap_are_precise() {
    let (mut positives, mut cuts, mut permissions) = (0, 0, 0);
    for kind in KINDS {
        for alias in 0..8 {
            let bytes = instruction(0xc0, kind, alias, 255);
            for pc in [0x1ffd, 0xffff_fffd, 0x1fff] {
                let engine = code(pc, &bytes, true);
                let memory = engine.memory().unwrap();
                let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &expected(kind, BYTES[alias as usize], 255)
                );
                assert_eq!(
                    (decoded.length(), decoded.next_pc()),
                    (3, GuestAddress(pc.wrapping_add(3)))
                );
                assert!(memory.is_code_current(decoded.code_snapshot()));
                if pc == 0x1ffd {
                    assert!(
                        memory
                            .resolve(GuestAddress(0x2000), Access::Execute)
                            .is_err()
                    );
                }
                if pc == 0xffff_fffd {
                    assert!(memory.resolve(GuestAddress(0), Access::Execute).is_err());
                    for artifact in [
                        compile_region(memory, &[spec(pc, 3)], CompileLimits::default()).unwrap(),
                        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                            .unwrap(),
                    ] {
                        assert_eq!(artifact.metadata().instructions, 1);
                    }
                }
                positives += 1;
            }
            for length in [1, 2] {
                for top in [false, true] {
                    let pc = if top {
                        ((1_u64 << 32) - length as u64) as u32
                    } else {
                        0x2000 - length as u32
                    };
                    let engine = code(pc, &bytes[..length], true);
                    crate::support_fetch::assert_decode_contract(
                        (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                            .expect_err("instruction must fail to decode"),
                        fetch_error(
                            pc,
                            if top { pc } else { 0x2000 },
                            length as u32 + 1,
                            if top {
                                FaultReason::AddressOverflow
                            } else {
                                FaultReason::Unmapped
                            },
                        ),
                    );
                    cuts += 1;
                    if !top {
                        let mut engine = code(pc, &bytes, false);
                        engine.protect(0x2000, 1, 1).unwrap();
                        crate::support_fetch::assert_decode_contract(
                            (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                                .expect_err("instruction must fail to decode"),
                            fetch_error(pc, 0x2000, length as u32 + 1, FaultReason::Permission),
                        );
                        permissions += 1;
                    }
                }
            }
            let mut engine = code(CODE, &bytes, false);
            engine.protect(CODE, 1, 1).unwrap();
            crate::support_fetch::assert_decode_contract(
                (decode_one(engine.memory().unwrap(), GuestAddress(CODE)))
                    .expect_err("instruction must fail to decode"),
                fetch_error(CODE, CODE, 1, FaultReason::Permission),
            );
            permissions += 1;
            let engine = code(CODE, &bytes, true);
            for length in [1, 2] {
                crate::support_fetch::assert_compile_contract(
                    prepare_region(
                        engine.memory().unwrap(),
                        &[spec(CODE, length)],
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
    }
    assert_eq!((positives, cuts, permissions), (48, 64, 48));
}

#[test]
fn each_consumed_byte_versions_same_and_changed_uploads_without_data_currency() {
    let mut invalidations = 0;
    for kind in KINDS {
        for alias in [0, 1, 5] {
            let bytes = instruction(0xc0, kind, alias, 9);
            for changed in 0..3 {
                for same in [false, true] {
                    for entries in [false, true] {
                        let pc = 0x1ffe;
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
                        } else {
                            match changed {
                                0 => 0xd0,
                                1 => bytes[1] ^ 8,
                                _ => 1,
                            }
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
                        invalidations += 1;
                    }
                }
            }
        }
    }
    assert_eq!(invalidations, 72);
    let mut equivalent = 0;
    for kind in KINDS {
        for alias in [0, 1, 5] {
            for entries in [false, true] {
                let mut bytes = instruction(0xc0, kind, alias, 1);
                bytes.extend([0xeb, 0]);
                let mut engine = code(CODE, &bytes, false);
                let old = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                let artifact = compile_region(
                    engine.memory().unwrap(),
                    &[spec(CODE, 5)],
                    CompileLimits::default(),
                )
                .unwrap();
                describe(&mut engine, CODE, 5, entries);
                let generation = compile(&mut engine, false, entries).unwrap() as u32;
                describe(&mut engine, CODE, 5, entries);
                let resident = compile(&mut engine, true, entries).unwrap();
                upload(&mut engine, CODE + 2, &[33]);
                let current = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(old.operation(), current.operation());
                assert!(
                    !engine
                        .memory()
                        .unwrap()
                        .is_code_current(old.code_snapshot())
                );
                assert_eq!(
                    artifact.wasm_bytes(engine.memory().unwrap()),
                    Err(ArtifactError::CodeInvalidated)
                );
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
                equivalent += 1;
            }
        }
    }
    assert_eq!(equivalent, 12);
    for entries in [false, true] {
        let pc = 0x1ffb;
        let mut engine = code(pc, &[0xc0, 0xd0, 2, 0xeb, 0, 0xf0, 0x90], false);
        let memory = engine.memory().unwrap();
        let artifacts = [
            compile_region(memory, &[spec(pc, 5)], CompileLimits::default()).unwrap(),
            compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).unwrap(),
        ];
        describe(&mut engine, pc, 5, entries);
        let generation = compile(&mut engine, false, entries).unwrap() as u32;
        describe(&mut engine, pc, 5, entries);
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
fn default_caps_and_observed_module_byte_limits_remain_closed() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    for kind in KINDS {
        let atom = instruction(0xc0, kind, 1, 2);
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
        let mut engine = code(CODE, &bytes, true);
        retain_prior(
            &mut engine,
            CODE,
            bytes.len(),
            CompileError::InstructionLimit,
        );
    }
    let bytes = [0xc0, 0xd5, 10, 0xeb, 0];
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
            instructions: 257,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 0,
            ..limits
        },
        CompileLimits {
            wasm_bytes: 524_289,
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
    let atom = [0xc0, 0xd0, 2, 0xeb, 0];
    let bytes = atom.repeat(9);
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    let specs: Vec<_> = (0..9).map(|index| spec(CODE + index * 5, 5)).collect();
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
    let mut engine = code(CODE, &bytes, true);
    describe(&mut engine, CODE, 5, false);
    let generation = engine.compile(1).unwrap();
    let mut units = Vec::new();
    for index in 0..8 {
        describe(&mut engine, CODE + index * 5, 5, false);
        let id = engine.compile_resident(1).unwrap().get();
        engine
            .acknowledge_resident_installation(KEY, id, index)
            .unwrap();
        units.push(id);
    }
    for entries in [false, true] {
        describe(&mut engine, CODE + 40, 5, entries);
        let arena = engine.arena().to_vec();
        let arena_address = engine.arena_address();
        let modules: Vec<_> = units
            .iter()
            .map(|id| {
                let bytes = engine.resident_bytes(*id).unwrap();
                (*id, bytes.to_vec(), bytes.as_ptr() as usize)
            })
            .collect();
        let artifact = engine.artifact_bytes().unwrap();
        let artifact = (artifact.to_vec(), artifact.as_ptr() as usize);
        let dispatcher = engine.dispatcher_bytes(KEY).unwrap();
        let dispatcher = (dispatcher.to_vec(), dispatcher.as_ptr() as usize);
        let code_before = page(engine.memory().unwrap(), CODE);
        crate::support_fetch::assert_host_contract(
            (compile(&mut engine, true, entries))
                .expect_err("compilation must refuse this fixture"),
            HostError::Resident(RegistryError::UnitCapacity),
        );
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.arena_address(), arena_address);
        assert_eq!(engine.generation(), generation);
        let current = engine.artifact_bytes().unwrap();
        assert_eq!((current.to_vec(), current.as_ptr() as usize), artifact);
        let current = engine.dispatcher_bytes(KEY).unwrap();
        assert_eq!((current.to_vec(), current.as_ptr() as usize), dispatcher);
        assert_eq!(page(engine.memory().unwrap(), CODE), code_before);
        for (index, (id, bytes, pointer)) in modules.iter().enumerate() {
            let current = engine.resident_bytes(*id).unwrap();
            assert_eq!(current, bytes);
            assert_eq!(current.as_ptr() as usize, *pointer);
            assert_eq!(
                engine
                    .lookup_resident(CODE + index as u32 * 5)
                    .unwrap()
                    .get(),
                *id
            );
            assert_eq!(
                engine
                    .lookup_installed_resident(KEY, CODE + index as u32 * 5)
                    .unwrap()
                    .unit_id,
                *id
            );
            engine.guard_resident(KEY, *id).unwrap();
        }
        engine.guard(KEY, generation).unwrap();
        assert!(engine.lookup_resident(CODE + 40).is_err());
    }
}
