use ring3_engine::{
    cpu::{
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{EffectiveAddress, Location32, Operation, ShiftCount, ShiftKind},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

#[test]
fn bound_memory_shl_admits_without_reading_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0xd1, 0x25, 0x10, 0x50, 0, 0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&code);
    engine.upload(0x1000, 8).unwrap();
    engine.protect(0x1000, 1, 5).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x5010), Access::Read)
            .is_err()
    );
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&8_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("bound memory SHL must admit without reading guest data");
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const FORMS: [&[u8]; 15] = [
    &[0xd1, 0x25, 0x10, 0x50, 0, 0],
    &[0xc1, 0x25, 0x10, 0x50, 0, 0, 0xff],
    &[0xd3, 0x25, 0x10, 0x50, 0, 0],
    &[0xd1, 0x2d, 0x10, 0x50, 0, 0],
    &[0xc1, 0x2d, 0x10, 0x50, 0, 0, 0x21],
    &[0xd3, 0x2d, 0x10, 0x50, 0, 0],
    &[0xd1, 0x3d, 0x10, 0x50, 0, 0],
    &[0xc1, 0x3d, 0x10, 0x50, 0, 0, 0x1f],
    &[0xd3, 0x3d, 0x10, 0x50, 0, 0],
    &[0xc1, 0x25, 0x10, 0x50, 0, 0, 0],
    &[0xc1, 0x2d, 0x10, 0x50, 0, 0, 0],
    &[0xc1, 0x3d, 0x10, 0x50, 0, 0, 0],
    &[0xc1, 0x25, 0x10, 0x50, 0, 0, 0x20],
    &[0xc1, 0x2d, 0x10, 0x50, 0, 0, 0x20],
    &[0xc1, 0x3d, 0x10, 0x50, 0, 0, 0x20],
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
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)], entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[140..140 + ring3_engine::abi::arena::TRANSFER_SIZE];
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

fn assert_rmw_module(engine: &EngineInstance, owner: Owner, id: u64) {
    let wasm = module(engine, owner, id);
    let resident = matches!(owner, Owner::Resident);
    crate::support_wasm_interface::assert_bound(
        wasm,
        resident,
        &[
            ("cpu_read32", 1),
            (
                if resident {
                    "cpu_store_resident32"
                } else {
                    "cpu_store32"
                },
                if resident { 6 } else { 2 },
            ),
            ("cpu_read_modify32", 1),
        ],
        &[
            4,
            if resident { 7 } else { 6 },
            1,
            if resident { 6 } else { 2 },
        ],
        true,
    );
    crate::support_wasm_interface::assert_binding_prefix(wasm, KEY, id, resident);
}

#[test]
fn nine_forms_and_both_masked_zero_immediates_import_full_read_and_owned_store() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let before = engine.arena().to_vec();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA + 0x10), Access::Read)
                        .is_err()
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(CODE), Access::Read)
                        .is_err()
                );
                assert_rmw_module(&engine, owner, id);
                guard(&engine, owner, id).unwrap();
                match owner {
                    Owner::Replacement => assert_eq!(engine.generation(), id as u32),
                    Owner::Resident => {
                        assert_eq!(engine.generation(), 0);
                        for pc in [CODE, CODE + instruction.len() as u32] {
                            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn original_ecx_esp_and_no_base_descriptions_keep_memory_destination_and_count() {
    let cases: &[(&[u8], ShiftKind, EffectiveAddress, ShiftCount)] = &[
        (
            &[0xd3, 0x64, 0x49, 0xe0],
            ShiftKind::Shl,
            EffectiveAddress {
                base: Some(Register32::Ecx),
                index: Some(Register32::Ecx),
                scale: 2,
                displacement: 0xffff_ffe0,
                ..Default::default()
            },
            ShiftCount::Cl,
        ),
        (
            &[0xd1, 0x7c, 0x24, 8],
            ShiftKind::Sar,
            EffectiveAddress {
                base: Some(Register32::Esp),
                index: None,
                scale: 1,
                displacement: 8,
                ..Default::default()
            },
            ShiftCount::Immediate(1),
        ),
        (
            &[0xc1, 0x2c, 0x8d, 0xf8, 0xff, 0xff, 0xff, 0x20],
            ShiftKind::Shr,
            EffectiveAddress {
                base: None,
                index: Some(Register32::Ecx),
                scale: 4,
                displacement: 0xffff_fff8,
                ..Default::default()
            },
            ShiftCount::Immediate(32),
        ),
        (
            &[0xc1, 0x3d, 0xff, 0xff, 0xff, 0xff, 0],
            ShiftKind::Sar,
            EffectiveAddress {
                base: None,
                index: None,
                scale: 1,
                displacement: u32::MAX,
                ..Default::default()
            },
            ShiftCount::Immediate(0),
        ),
    ];
    for &(instruction, kind, address, count) in cases {
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
            &Operation::Shift {
                kind,
                destination: Location32::Memory(address),
                count,
            }
        );
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, &[(CODE, bytes.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_rmw_module(&engine, owner, id);
            }
        }
    }
}

#[test]
fn page_end_terminal_jump_does_not_fetch_successor_or_guest_data() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        let pc = 0x2000 - bytes.len() as u32;
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(pc, &bytes, true);
                describe(&mut engine, &[(pc, bytes.len())], entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_eq!(engine.arena(), before);
                assert_rmw_module(&engine, owner, id);
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

fn saved(engine: &EngineInstance, keep: u64) -> Saved {
    let replacement = engine.artifact_bytes().unwrap();
    let resident = engine.resident_bytes(keep).unwrap();
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

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(KEEP, 3)], false);
    engine.compile(1).unwrap();
    describe(&mut engine, &[(KEEP, 3)], false);
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn assert_retained(engine: &EngineInstance, keep: u64, before: &Saved) {
    assert_eq!(&saved(engine, keep), before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

#[test]
fn missing_memory_count_byte_faults_at_execute_boundary_without_publication() {
    for modrm in [0x25, 0x2d, 0x3d] {
        let missing = [0xc1, modrm, 0x10, 0x50, 0, 0];
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
                length: 7,
            }),
        );
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(KEEP), 3)
                    .unwrap();
                upload(&mut engine, pc, &missing);
                engine.protect(CODE, 1, 4).unwrap();
                describe(&mut engine, &[(pc, 7)], entries);
                let before = saved(&engine, keep);
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, owner, entries, 1))
                        .expect_err("compilation must refuse this fixture"),
                    compile_error(owner, error),
                );
                assert_retained(&engine, keep, &before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
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
fn consumed_code_pages_invalidate_but_data_mapping_and_writes_do_not() {
    let instruction = [0xc1, 0x25, 0x10, 0x50, 0, 0, 0x20];
    let mut bytes = instruction.to_vec();
    bytes.extend([0xeb, 0]);
    let pc = 0x1ffc;
    for offset in [0, 4, 6] {
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
                engine.write32(DATA + 0x10, 0xffff_fff7).unwrap();
                engine.protect(DATA, 1, 1).unwrap();
                engine.unmap(DATA, 1).unwrap();
                assert_eq!(module(&engine, owner, id), preserved);
                assert_eq!(module(&engine, owner, id).as_ptr(), pointer);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                guard(&engine, owner, id).unwrap();
                upload(&mut engine, pc + offset as u32, &[instruction[offset]]);
                assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                let expected = match owner {
                    Owner::Replacement => HostError::CodeInvalidated,
                    Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
                };
                crate::support_fetch::assert_host_contract(
                    (guard(&engine, owner, id)).expect_err("compilation must refuse this fixture"),
                    expected,
                );
                match owner {
                    Owner::Replacement => assert_eq!(engine.artifact_bytes(), Err(expected)),
                    Owner::Resident => assert_eq!(engine.resident_bytes(id), Err(expected)),
                }
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
            }
        }
    }
}

#[test]
fn operand_splitting_extents_or_seeds_fail_before_replacing_either_prior_owner() {
    for instruction in FORMS {
        let mut bytes = instruction.to_vec();
        bytes.extend([0xeb, 0]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &bytes);
                let specs = if entries {
                    vec![(CODE, 0), (CODE + instruction.len() as u32 - 1, 0)]
                } else {
                    vec![(CODE, instruction.len() - 1)]
                };
                describe(&mut engine, &specs, entries);
                let before = saved(&engine, keep);
                let expected = if entries {
                    CompileError::InvalidBlocks
                } else {
                    instruction_error(CODE, InstructionError::InvalidBlockEnd)
                };
                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, owner, entries, specs.len() as u32))
                        .expect_err("compilation must refuse this fixture"),
                    compile_error(owner, expected),
                );
                assert_retained(&engine, keep, &before);
            }
        }
    }
}

#[test]
fn adjacent_memory_entries_keep_exact_instruction_and_terminal_jump_boundaries() {
    let bytes = [0xd1, 0x23, 0xc1, 0x7c, 0x24, 8, 0, 0xeb, 0, 0xf0, 0x90];
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &bytes, true);
            describe(&mut engine, &[(CODE, 2), (CODE + 2, 7)], entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries, 2).unwrap();
            assert_eq!(engine.arena(), before);
            assert_rmw_module(&engine, owner, id);
            if matches!(owner, Owner::Resident) {
                for pc in [CODE, CODE + 2, CODE + 7] {
                    assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                }
                assert!(engine.lookup_resident(CODE + 9).is_err());
            }
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(DATA), Access::Read)
                    .is_err()
            );
        }
    }
}

#[test]
fn memory_shift_charges_default_sixty_four_limit_before_poison_atomically() {
    for instruction in [FORMS[0], FORMS[9], FORMS[12]] {
        let mut admitted = vec![0x90; 62];
        admitted.extend_from_slice(instruction);
        admitted.extend([0xeb, 0]);
        let mut excessive = vec![0x90; 63];
        excessive.extend_from_slice(instruction);
        excessive.extend([0xf0, 0x90]);
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &admitted);
                describe(&mut engine, &[(CODE, admitted.len())], entries);
                let id = compile(&mut engine, owner, entries, 1).unwrap();
                assert_rmw_module(&engine, owner, id);
                guard(&engine, owner, id).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &excessive);
                describe(&mut engine, &[(CODE, excessive.len())], entries);
                let before = saved(&engine, keep);
                if entries
                    && crate::support_admission::admit_if_deferred(
                        &mut engine,
                        crate::support_admission::ColdEntryCase {
                            key: KEY,
                            keeper: keep,
                            resident: matches!(owner, Owner::Resident),
                            pages: &[CODE, KEEP],
                            expected: CompileError::InstructionLimit,
                        },
                    )
                {
                    continue;
                }

                crate::support_fetch::assert_host_contract(
                    (compile(&mut engine, owner, entries, 1))
                        .expect_err("compilation must refuse this fixture"),
                    compile_error(owner, CompileError::InstructionLimit),
                );
                assert_retained(&engine, keep, &before);
            }
        }
    }
}
