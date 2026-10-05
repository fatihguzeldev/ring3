use ring3_engine::process::EngineInstance;

#[test]
fn register_byte_logical_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x24, 0x80, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register byte logical must admit in the bound engine");
}

use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            ArtifactError, BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region, prepare_entry_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteLogicalKind, ByteRegister, ByteValue, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const KEY: u64 = 0x8abc_def0_f123_4567;
const REGISTERS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
const LOGICAL: [(ByteLogicalKind, u8, u8, u8, u8); 3] = [
    (ByteLogicalKind::And, 0x20, 0x22, 0x24, 4),
    (ByteLogicalKind::Or, 0x08, 0x0a, 0x0c, 1),
    (ByteLogicalKind::Xor, 0x30, 0x32, 0x34, 6),
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

fn host_error(owner: Owner, error: CompileError) -> HostError {
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

fn forms() -> Vec<(Vec<u8>, Operation)> {
    let mut forms = Vec::new();
    for (kind, to_rm, to_reg, al, extension) in LOGICAL {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (source_index, source) in REGISTERS.into_iter().enumerate() {
                for (opcode, modrm) in [
                    (
                        to_rm,
                        0xc0 | (source_index as u8) << 3 | destination_index as u8,
                    ),
                    (
                        to_reg,
                        0xc0 | (destination_index as u8) << 3 | source_index as u8,
                    ),
                ] {
                    forms.push((
                        vec![opcode, modrm],
                        Operation::LogicalByte {
                            kind,
                            destination,
                            source: ByteValue::Register(source),
                        },
                    ));
                }
            }
            for immediate in [0, 0x7f, 0x80, 0xff] {
                forms.push((
                    vec![
                        0x80,
                        0xc0 | extension << 3 | destination_index as u8,
                        immediate,
                    ],
                    Operation::LogicalByte {
                        kind,
                        destination,
                        source: ByteValue::Immediate(immediate),
                    },
                ));
            }
        }
        for immediate in [0, 0x7f, 0x80, 0xff] {
            forms.push((
                vec![al, immediate],
                Operation::LogicalByte {
                    kind,
                    destination: ByteRegister::Al,
                    source: ByteValue::Immediate(immediate),
                },
            ));
        }
    }
    assert_eq!(forms.len(), 492);
    forms
}

fn excluded() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut forms = Vec::new();
    for (_, to_rm, to_reg, al, extension) in LOGICAL {
        for bytes in [
            vec![0x66, to_rm, 0x00],
            vec![0x66, to_reg, 0x00],
            vec![0x66, 0x80, extension << 3, 0xff],
            vec![0x82, 0xc0 | extension << 3, 0xff],
            vec![0x66, to_rm, 0xc0],
            vec![0x66, to_rm + 1, 0xc0],
            vec![0x67, to_reg, 0xc0],
            vec![0xf2, al, 0xff],
            vec![0xf3, 0x80, 0xc0 | extension << 3, 0xff],
            vec![0xf0, to_rm, 0x00],
        ] {
            forms.push((bytes, opcode));
        }
        forms.push((vec![0xf0, to_rm, 0xc0], DecodeError::InvalidEncoding));
        forms.push((
            vec![0x64, to_rm, 0xc0],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ));
    }
    for bytes in [
        vec![0x66, 0x00, 0x00],
        vec![0x66, 0x10, 0x00],
        vec![0x66, 0x28, 0x00],
        vec![0x66, 0x18, 0x00],
    ] {
        forms.push((bytes, opcode));
    }
    forms
}

#[test]
fn all_twelve_families_decode_both_directions_aliases_and_unsigned_immediates() {
    let mut engine = code(CODE, &[0x90], false);
    for (bytes, expected) in forms() {
        upload(&mut engine, CODE, &bytes);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(decoded.operation(), &expected, "{bytes:02x?}");
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + bytes.len() as u32));
    }
}

#[test]
fn canonical_batches_admit_standalone_and_four_bound_profiles_with_existing_caps() {
    for chunk in forms().chunks(63) {
        let mut bytes = Vec::new();
        let mut starts = Vec::new();
        for (instruction, _) in chunk {
            starts.push(CODE + bytes.len() as u32);
            bytes.extend(instruction);
        }
        starts.push(CODE + bytes.len() as u32);
        bytes.extend([0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        for region in [
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap(),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
        ] {
            assert_eq!(
                (region.metadata().blocks, region.metadata().instructions),
                (1, chunk.len() + 1)
            );
            assert_eq!(&region.wasm_bytes(memory).unwrap()[..8], b"\0asm\x01\0\0\0");
        }
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes, true);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                match owner {
                    Owner::Replacement => engine.guard(KEY, id as u32).unwrap(),
                    Owner::Resident => {
                        engine.guard_resident(KEY, id).unwrap();
                        for &pc in &starts {
                            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                        }
                        assert!(engine.lookup_resident(CODE + 1).is_err());
                        assert_eq!(engine.generation(), 0);
                    }
                }
            }
        }
    }
    let mut bytes = [0x24, 0x80].repeat(64);
    bytes.extend([0xeb, 0]);
    let engine = code(CODE, &bytes, true);
    for error in [
        compile_region(
            engine.memory().unwrap(),
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .err(),
        compile_entry_region(
            engine.memory().unwrap(),
            &[GuestAddress(CODE)],
            CompileLimits::default(),
        )
        .err(),
    ] {
        assert_eq!(error, Some(CompileError::InstructionLimit));
    }
    let engine = code(CODE, &[0x24, 0x80, 0xeb, 0], true);
    for (limits, expected) in [
        (
            CompileLimits {
                instructions: 1,
                ..CompileLimits::default()
            },
            CompileError::InstructionLimit,
        ),
        (
            CompileLimits {
                wasm_bytes: 1,
                ..CompileLimits::default()
            },
            CompileError::WasmLimit,
        ),
    ] {
        assert_eq!(
            compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: 4
                }],
                limits
            )
            .err(),
            Some(expected)
        );
        assert_eq!(
            compile_entry_region(engine.memory().unwrap(), &[GuestAddress(CODE)], limits).err(),
            Some(expected)
        );
    }
}

#[test]
fn memory_alternate_prefix_word_and_adjacent_forms_keep_exact_refusal_categories() {
    for (bytes, expected) in excluded() {
        let engine = code(CODE, &bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        let error = instruction_error(CODE, InstructionError::Decode(expected));
        assert_eq!(
            compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32
                }],
                CompileLimits::default()
            )
            .err(),
            Some(error)
        );
        assert_eq!(
            compile_entry_region(
                engine.memory().unwrap(),
                &[GuestAddress(CODE)],
                CompileLimits::default()
            )
            .err(),
            Some(error)
        );
    }
}

#[test]
fn exact_fetch_wrap_and_cross_page_snapshots_keep_their_boundaries() {
    for (_, to_rm, to_reg, al, extension) in LOGICAL {
        for bytes in [
            vec![to_rm, 0xe0],
            vec![to_reg, 0xe0],
            vec![al, 0xff],
            vec![0x80, 0xc7 | extension << 3, 0x80],
        ] {
            let pc = 0x2000 - bytes.len() as u32;
            let engine = code(pc, &bytes, true);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length() as usize, bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            let truncated = &bytes[..bytes.len() - 1];
            let pc = 0x2000 - truncated.len() as u32;
            let engine = code(pc, truncated, true);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(0x2000),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped
                    },
                    length: bytes.len() as u32,
                })
            );
        }
        let engine = code(u32::MAX - 1, &[al, 0xff], true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX - 1))
                .unwrap()
                .next_pc(),
            GuestAddress(0)
        );
        let engine = code(u32::MAX, &[al], true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(u32::MAX),
                fault: MemoryFault {
                    address: GuestAddress(u32::MAX),
                    access: Access::Execute,
                    reason: FaultReason::AddressOverflow
                },
                length: 2,
            })
        );
    }
    for changed in [0x1fff, 0x2000] {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(0x1fff, &[0x20, 0xe0, 0xeb, 0], false);
                let prepared = prepare_entry_region(
                    engine.memory().unwrap(),
                    &[GuestAddress(0x1fff)],
                    CompileLimits::default(),
                )
                .unwrap();
                let region = compile_entry_region(
                    engine.memory().unwrap(),
                    &[GuestAddress(0x1fff)],
                    CompileLimits::default(),
                )
                .unwrap();
                describe(&mut engine, 0x1fff, 4, entries);
                let id = compile(&mut engine, owner, entries).unwrap();
                engine.map(0x4000, 1, 7).unwrap();
                upload(&mut engine, 0x4000, &[0x90]);
                assert!(prepared.is_current(engine.memory().unwrap()));
                region.wasm_bytes(engine.memory().unwrap()).unwrap();
                engine
                    .write8(changed, if changed == 0x1fff { 0x20 } else { 0xe0 })
                    .unwrap();
                assert!(!prepared.is_current(engine.memory().unwrap()));
                assert_eq!(
                    region.wasm_bytes(engine.memory().unwrap()),
                    Err(ArtifactError::CodeInvalidated)
                );
                let error = match owner {
                    Owner::Replacement => engine.guard(KEY, id as u32),
                    Owner::Resident => engine.guard_resident(KEY, id),
                };
                assert_eq!(
                    error,
                    Err(match owner {
                        Owner::Replacement => HostError::CodeInvalidated,
                        Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
                    })
                );
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    replacement: (Vec<u8>, usize),
    resident: (Vec<u8>, usize),
    dispatcher: (Vec<u8>, usize),
    ram: [Vec<u8>; 2],
    mapped_pages: u32,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    let retain = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Publication {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        replacement: retain(engine.artifact_bytes().unwrap()),
        resident: retain(engine.resident_bytes(id).unwrap()),
        dispatcher: retain(engine.dispatcher_bytes(KEY).unwrap()),
        ram: [CODE, KEEP].map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        mapped_pages: engine.memory().unwrap().mapped_pages(),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = code(CODE, &[0x90], false);
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn failed(
    engine: &mut EngineInstance,
    keep: u64,
    owner: Owner,
    entries: bool,
    pc: u32,
    length: usize,
    expected: CompileError,
) {
    describe(engine, pc, length, entries);
    let before = publication(engine, keep);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), 4096)
        .unwrap();
    assert_eq!(
        compile(engine, owner, entries),
        Err(host_error(owner, expected))
    );
    assert_eq!(publication(engine, keep), before);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

#[test]
fn failed_late_decode_fetch_and_cap_preparation_preserves_both_published_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let (mut engine, keep) = prior_owners();
            for (instruction, error) in excluded() {
                let mut bytes = vec![0x24, 0xff];
                bytes.extend(instruction);
                upload(&mut engine, CODE, &bytes);
                failed(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    bytes.len(),
                    instruction_error(CODE + 2, InstructionError::Decode(error)),
                );
            }
            upload(&mut engine, 0x1ffe, &[0x80, 0xe7]);
            failed(
                &mut engine,
                keep,
                owner,
                entries,
                0x1ffe,
                3,
                instruction_error(
                    0x1ffe,
                    InstructionError::Decode(DecodeError::MemoryFault {
                        pc: GuestAddress(0x1ffe),
                        fault: MemoryFault {
                            address: GuestAddress(0x2000),
                            access: Access::Execute,
                            reason: FaultReason::Unmapped,
                        },
                        length: 3,
                    }),
                ),
            );
            let mut bytes = [0x24, 0x80].repeat(64);
            bytes.extend([0xeb, 0]);
            upload(&mut engine, CODE, &bytes);
            failed(
                &mut engine,
                keep,
                owner,
                entries,
                CODE,
                bytes.len(),
                CompileError::InstructionLimit,
            );
        }
    }
}
