use ring3_engine::process::EngineInstance;

#[test]
fn leave32_admits_before_a_following_jump_in_the_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0xc9, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&code);
    engine.upload(0x1000, 3).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&3_u32.to_le_bytes());
    assert_eq!(
        engine.compile(1),
        Ok(1),
        "leave32 must admit sequentially before the block's jump"
    );
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region, prepare_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{HostError, ResidentInstallation},
};
use wasm_encoder::{
    EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType, Module,
    TypeSection, ValType,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;

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
    let mut engine = EngineInstance::new(pages + 3, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    if execute_only {
        engine.protect(base, pages, 4).unwrap();
    }
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, owner: Owner, entries: bool) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(1).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(1, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(1).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn failure(owner: Owner, error: CompileError) -> HostError {
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

fn declarations(owner: Owner) -> Vec<u8> {
    let resident = matches!(owner, Owner::Resident);
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    types.ty().function(
        std::iter::repeat_n(ValType::I32, if resident { 7 } else { 6 }),
        [ValType::I32],
    );
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    imports.import(
        "ring3",
        if resident { "guard_resident" } else { "guard" },
        EntityType::Function(1),
    );
    imports.import("ring3", "read32", EntityType::Function(2));
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, 2);
    module.section(&exports);
    // this prefix proves exact guard/read32 declarations, not instruction bodies.
    module.finish()
}

#[test]
fn exact_leave_fetches_one_byte_at_page_and_guest_address_ends() {
    for pc in [CODE, 0x1fff, u32::MAX] {
        let mut engine = code(pc, &[0xc9], true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.operation(), &Operation::Leave);
        assert_eq!(decoded.pc(), GuestAddress(pc));
        assert_eq!(decoded.length(), 1);
        assert_eq!(decoded.next_pc(), GuestAddress(pc.wrapping_add(1)));
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        engine.protect(pc & !0xfff, 1, 1).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(pc),
                    access: Access::Execute,
                    reason: FaultReason::Permission,
                },
                length: 1,
            })
        );
    }
    let engine = code(0x1fff, &[0xc9], true);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(0x2000)).err(),
        Some(DecodeError::MemoryFault {
            pc: GuestAddress(0x2000),
            fault: MemoryFault {
                address: GuestAddress(0x2000),
                access: Access::Execute,
                reason: FaultReason::Unmapped,
            },
            length: 1,
        })
    );
}

#[test]
fn four_bound_profiles_keep_leave_sequential_and_require_only_checked_read32() {
    for bytes in [
        &[0xc9, 0x90, 0xeb, 0][..],
        &[0x90, 0xc9, 0xc3][..],
        &[0xc9, 0xc9, 0xeb, 0][..],
    ] {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, bytes, true);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), bytes.len())
                    .unwrap();
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let pointer = engine.arena_address();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.arena_address(), pointer);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0), Access::Read)
                        .is_err()
                );
                let mut fetched = vec![0; bytes.len()];
                engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(CODE), &mut fetched)
                    .unwrap();
                assert_eq!(fetched, bytes);
                let artifact = match owner {
                    Owner::Replacement => {
                        engine.guard(KEY, id as u32).unwrap();
                        engine.artifact_bytes().unwrap()
                    }
                    Owner::Resident => {
                        engine.guard_resident(KEY, id).unwrap();
                        for offset in 0..3 {
                            assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                        }
                        if bytes.len() == 4 {
                            assert!(engine.lookup_resident(CODE + 3).is_err());
                        }
                        engine.resident_bytes(id).unwrap()
                    }
                };
                assert!(artifact.starts_with(&declarations(owner)));
            }
        }
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut engine = code(u32::MAX, &[0xc9], true);
        describe(&mut engine, u32::MAX, 1, false);
        assert!(compile(&mut engine, owner, false).is_ok());
    }
}

#[test]
fn standalone_and_prefixed_or_adjacent_forms_keep_explicit_refusals() {
    for bytes in [&[0x90, 0xc9][..], &[0x90, 0xc9, 0xeb, 0][..]] {
        let engine = code(CODE, bytes, true);
        let memory = engine.memory().unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let error = instruction_error(CODE + 1, InstructionError::BackendUnsupported);
        for observed in [
            prepare_region(memory, &specs, CompileLimits::default()).err(),
            prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            compile_region(memory, &specs, CompileLimits::default()).err(),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        ] {
            assert_eq!(observed, Some(error));
        }
    }
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut refused = vec![
        (vec![0x66, 0xc9], opcode),
        (vec![0x67, 0xc9], opcode),
        (vec![0xf2, 0xc9], opcode),
        (vec![0xf3, 0xc9], opcode),
        (vec![0xf0, 0xc9], DecodeError::InvalidEncoding),
        (vec![0xc8, 0, 0, 0], opcode),
        (vec![0xcb], opcode),
        (vec![0xca, 0, 0], opcode),
    ];
    for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        refused.push((
            vec![prefix, 0xc9],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ));
    }
    assert_eq!(refused.len(), 14);
    for (bytes, expected) in refused {
        let engine = code(CODE, &bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped: engine.memory().unwrap().mapped_pages(),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

#[test]
fn late_refusal_and_missing_next_instruction_preserve_both_published_owners() {
    for (pc, bytes) in [
        (CODE, &[0xc9, 0x66, 0xc9][..]),
        (CODE, &[0xc9, 0xc8, 0, 0, 0][..]),
        (0x1fff, &[0xc9][..]),
    ] {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                if pc == 0x1fff && !entries {
                    continue;
                }
                let mut engine = code(pc, bytes, false);
                engine.map(KEEP, 1, 7).unwrap();
                upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                engine.map(DATA, 1, 3).unwrap();
                engine.write32(DATA, 0x9234_5678).unwrap();
                describe(&mut engine, KEEP, 3, false);
                engine.compile(1).unwrap();
                let keep = engine.compile_resident(1).unwrap().get();
                engine
                    .acknowledge_resident_installation(KEY, keep, 3)
                    .unwrap();
                describe(&mut engine, pc, bytes.len(), entries);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 4096)
                    .unwrap();
                let before = publication(&engine, keep);
                let error = if pc == 0x1fff {
                    instruction_error(
                        0x2000,
                        InstructionError::Decode(DecodeError::MemoryFault {
                            pc: GuestAddress(0x2000),
                            fault: MemoryFault {
                                address: GuestAddress(0x2000),
                                access: Access::Execute,
                                reason: FaultReason::Unmapped,
                            },
                            length: 1,
                        }),
                    )
                } else {
                    instruction_error(
                        CODE + 1,
                        InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    )
                };
                assert_eq!(
                    compile(&mut engine, owner, entries),
                    Err(failure(owner, error))
                );
                assert_eq!(publication(&engine, keep), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(pc).is_err());
            }
        }
    }
}
