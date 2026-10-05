use ring3_engine::process::EngineInstance;

#[test]
fn register_set_byte_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0x94, 0xc0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
    engine.upload(0x1000, 5).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&5_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register set byte must admit in the bound engine");
}

use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, Condition, Operation},
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
const CONDITIONS: [Condition; 16] = [
    Condition::Overflow,
    Condition::NotOverflow,
    Condition::Below,
    Condition::AboveOrEqual,
    Condition::Equal,
    Condition::NotEqual,
    Condition::BelowOrEqual,
    Condition::Above,
    Condition::Sign,
    Condition::NotSign,
    Condition::Parity,
    Condition::NotParity,
    Condition::Less,
    Condition::GreaterOrEqual,
    Condition::LessOrEqual,
    Condition::Greater,
];

#[derive(Clone, Copy, Debug)]
enum Owner {
    Replacement,
    Resident,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
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
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
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

fn compile_error(owner: Owner, pc: u32, cause: InstructionError) -> HostError {
    let error = CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    };
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    replacement: Vec<u8>,
    replacement_pointer: usize,
    resident: Vec<u8>,
    resident_pointer: usize,
    mapped_pages: u32,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    let replacement = engine.artifact_bytes().unwrap();
    let resident = engine.resident_bytes(id).unwrap();
    Publication {
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
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

#[test]
fn all_conditions_destinations_and_ignored_reg_fields_have_exact_ir() {
    let mut engine = code(CODE, &[0x90], false);
    let mut encodings = 0;
    for (condition_index, condition) in CONDITIONS.into_iter().enumerate() {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for ignored_reg in 0..8 {
                let bytes = [
                    0x0f,
                    0x90 + condition_index as u8,
                    0xc0 | ignored_reg << 3 | destination_index as u8,
                ];
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::SetByte {
                        condition,
                        destination
                    },
                    "{bytes:02x?}"
                );
                assert_eq!(decoded.length(), 3);
                assert_eq!(decoded.next_pc(), GuestAddress(CODE + 3));
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .is_code_current(decoded.code_snapshot())
                );
                encodings += 1;
            }
        }
    }
    assert_eq!(encodings, 1024);
}

#[test]
fn canonical_forms_and_selected_ignored_fields_admit_all_profiles() {
    let mut canonical = Vec::new();
    for condition in 0..16 {
        for destination in 0..8 {
            canonical.push([0x0f, 0x90 + condition, 0xc0 | destination]);
        }
    }
    assert_eq!(canonical.len(), 128);
    let mut batches: Vec<Vec<u8>> = canonical
        .chunks(32)
        .map(|forms| forms.iter().flatten().copied().collect())
        .collect();
    batches.push(
        (0..16)
            .flat_map(|condition| [0x0f, 0x90 + condition, 0xf8 | (condition & 7)])
            .collect(),
    );
    for mut bytes in batches {
        let set_count = bytes.len() / 3;
        bytes.extend_from_slice(&[0xeb, 0]);
        let engine = code(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        for compiled in [
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
                (compiled.metadata().blocks, compiled.metadata().instructions),
                (1, set_count + 1)
            );
            assert_eq!(
                &compiled.wasm_bytes(memory).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
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
                    Owner::Replacement => {
                        engine.guard(KEY, id as u32).unwrap();
                        assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
                    }
                    Owner::Resident => {
                        engine.guard_resident(KEY, id).unwrap();
                        for index in 0..=set_count {
                            assert_eq!(
                                engine
                                    .lookup_resident(CODE + (index * 3) as u32)
                                    .unwrap()
                                    .get(),
                                id
                            );
                        }
                        assert!(engine.lookup_resident(CODE + 1).is_err());
                        assert_eq!(engine.generation(), 0);
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                    }
                }
            }
        }
    }
}

#[test]
fn memory_targets_prefixes_and_adjacent_operations_fail_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut engine = code(CODE, &[0x90], false);
    for condition in 0..16 {
        let bytes = [0x66, 0x0f, 0x90 + condition, 0x03];
        upload(&mut engine, CODE, &bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(opcode),
            "{bytes:02x?}"
        );
    }
    for (bytes, expected) in [
        (&[0x66, 0x0f, 0x94, 0x3c, 0x24][..], opcode),
        (&[0x66, 0x0f, 0x94, 0x05, 0, 0x50, 0, 0][..], opcode),
        (&[0x66, 0x0f, 0x94, 0xc0][..], opcode),
        (&[0x67, 0x0f, 0x94, 0xc0][..], opcode),
        (&[0xf2, 0x0f, 0x94, 0xc0][..], opcode),
        (&[0xf3, 0x0f, 0x94, 0xc0][..], opcode),
        (&[0xf0, 0x0f, 0x94, 0xc0][..], DecodeError::InvalidEncoding),
        (
            &[0x64, 0x0f, 0x94, 0xc0][..],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
        (&[0x0f, 0x44, 0xc0][..], opcode),
        (&[0x86, 0xc0][..], opcode),
    ] {
        upload(&mut engine, CODE, bytes);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn exact_fetch_spans_truncation_permission_and_address_wrap_keep_categories() {
    for instruction in [[0x0f, 0x90, 0xc0], [0x0f, 0x94, 0xfc], [0x0f, 0x9f, 0xff]] {
        for pc in [0x1ffd, 0x1ffe] {
            let engine = code(pc, &instruction, true);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(decoded.length(), 3);
            assert_eq!(decoded.next_pc(), GuestAddress(pc + 3));
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            if pc == 0x1ffd {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
        }
    }
    for prefix in [&[0x0f][..], &[0x0f, 0x94]] {
        let pc = 0x2000 - prefix.len() as u32;
        let engine = code(pc, prefix, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped
                },
                length: prefix.len() as u32 + 1,
            })
        );
    }
    let pc = u32::MAX - 2;
    let engine = code(pc, &[0x0f, 0x94, 0xc0], true);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(pc))
            .unwrap()
            .next_pc(),
        GuestAddress(0)
    );
    compile_region(
        engine.memory().unwrap(),
        &[BlockSpec {
            entry: GuestAddress(pc),
            byte_length: 3,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    let pc = u32::MAX - 1;
    let engine = code(pc, &[0x0f, 0x94], true);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
        Some(DecodeError::MemoryFault {
            pc: GuestAddress(pc),
            fault: MemoryFault {
                address: GuestAddress(pc),
                access: Access::Execute,
                reason: FaultReason::AddressOverflow
            },
            length: 3,
        })
    );
    let mut engine = code(CODE, &[0x0f, 0x94, 0xc0], false);
    engine.protect(CODE, 1, 3).unwrap();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
        Some(DecodeError::MemoryFault {
            pc: GuestAddress(CODE),
            fault: MemoryFault {
                address: GuestAddress(CODE),
                access: Access::Execute,
                reason: FaultReason::Permission
            },
            length: 1,
        })
    );
}

#[test]
fn instruction_cap_and_failed_preparation_preserve_both_published_owners() {
    let mut at_limit = [0x0f, 0x94, 0xc0].repeat(63);
    at_limit.extend_from_slice(&[0xeb, 0]);
    let mut above_limit = [0x0f, 0x94, 0xc0].repeat(64);
    above_limit.extend_from_slice(&[0xeb, 0]);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &at_limit, true);
            describe(&mut engine, CODE, at_limit.len(), entries);
            compile(&mut engine, owner, entries).unwrap();
            for (pc, bytes, error) in [
                (
                    CODE,
                    &[0x0f, 0x94, 0xc0, 0x66, 0x0f, 0x94, 0x03][..],
                    compile_error(
                        owner,
                        CODE + 3,
                        InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Opcode,
                        )),
                    ),
                ),
                (
                    CODE,
                    &[0x0f, 0x94, 0xc0, 0x64, 0x0f, 0x94, 0xc0][..],
                    compile_error(
                        owner,
                        CODE + 3,
                        InstructionError::Decode(DecodeError::Unsupported(
                            UnsupportedFeature::Segment,
                        )),
                    ),
                ),
                (
                    0x1ffe,
                    &[0x0f, 0x94][..],
                    compile_error(
                        owner,
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
                ),
                (
                    CODE,
                    above_limit.as_slice(),
                    match owner {
                        Owner::Replacement => HostError::Compile(CompileError::InstructionLimit),
                        Owner::Resident => HostError::Resident(RegistryError::Compile(
                            CompileError::InstructionLimit,
                        )),
                    },
                ),
            ] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, pc, bytes);
                let length = if pc == 0x1ffe { 3 } else { bytes.len() };
                describe(&mut engine, pc, length, entries);
                let before = publication(&engine, keep);
                assert_eq!(compile(&mut engine, owner, entries), Err(error));
                assert_eq!(publication(&engine, keep), before);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(CODE).is_err());
            }
        }
    }
}
