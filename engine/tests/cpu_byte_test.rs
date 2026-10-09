use ring3_engine::process::EngineInstance;

#[test]
fn register_byte_test_admits_in_bound_engine() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x84, 0xc0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register byte test must admit in the bound engine");
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
            ir::{ByteRegister, ByteValue, Operation},
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
const CHAIN: [u8; 9] = [0x84, 0xe0, 0xa8, 0xff, 0xf6, 0xc7, 0x80, 0xeb, 0];

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
fn byte_test_decodes_all_alias_pairs_and_unsigned_immediates() {
    let mut engine = code(CODE, &[0x90], false);
    let mut canonical_forms = 0;
    for (left_index, left) in REGISTERS.into_iter().enumerate() {
        for (right_index, right) in REGISTERS.into_iter().enumerate() {
            let modrm = 0xc0 | (right_index as u8) << 3 | left_index as u8;
            upload(&mut engine, CODE, &[0x84, modrm]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::TestByte {
                    left,
                    right: ByteValue::Register(right),
                },
                "84 {modrm:02x}"
            );
            assert_eq!(decoded.length(), 2);
            assert_eq!(decoded.next_pc(), GuestAddress(CODE + 2));
            canonical_forms += 1;
        }
        for immediate in [0, 0x7f, 0x80, 0xff] {
            upload(
                &mut engine,
                CODE,
                &[0xf6, 0xc0 | left_index as u8, immediate],
            );
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
            assert_eq!(
                decoded.operation(),
                &Operation::TestByte {
                    left,
                    right: ByteValue::Immediate(immediate),
                }
            );
            assert_eq!(decoded.length(), 3);
            assert_eq!(decoded.next_pc(), GuestAddress(CODE + 3));
            canonical_forms += 1;
        }
    }
    for immediate in [0, 0x7f, 0x80, 0xff] {
        upload(&mut engine, CODE, &[0xa8, immediate]);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::TestByte {
                left: ByteRegister::Al,
                right: ByteValue::Immediate(immediate),
            }
        );
        assert_eq!(decoded.length(), 2);
        assert_eq!(decoded.next_pc(), GuestAddress(CODE + 2));
        canonical_forms += 1;
    }
    assert_eq!(canonical_forms, 100);
}

#[test]
fn test_chain_admits_standalone_and_all_four_bound_profiles_without_data() {
    let engine = code(CODE, &CHAIN, true);
    let memory = engine.memory().unwrap();
    for compiled in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: CHAIN.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (compiled.metadata().blocks, compiled.metadata().instructions),
            (1, 4)
        );
        assert_eq!(
            &compiled.wasm_bytes(memory).unwrap()[..8],
            b"\0asm\x01\0\0\0"
        );
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = code(CODE, &CHAIN, true);
            describe(&mut engine, CODE, CHAIN.len(), entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            match owner {
                Owner::Replacement => engine.guard(KEY, id as u32).unwrap(),
                Owner::Resident => {
                    engine.guard_resident(KEY, id).unwrap();
                    for offset in [0, 2, 4, 7] {
                        assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                    }
                    assert!(engine.lookup_resident(CODE + 1).is_err());
                    assert_eq!(engine.generation(), 0);
                    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                }
            }
        }
    }
}

#[test]
fn memory_byte_tests_and_excluded_forms_fail_closed() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for (bytes, expected) in [
        (&[0x66, 0x84, 0x03][..], opcode),
        (&[0x66, 0xf6, 0x03, 0xff][..], opcode),
        (&[0xf6, 0xc8, 0xff][..], opcode),
        (&[0x66, 0x66, 0x85, 0xc0][..], opcode),
        (&[0x66, 0x84, 0xc0][..], opcode),
        (&[0x67, 0x84, 0xe0][..], opcode),
        (&[0xf2, 0xa8, 0xff][..], opcode),
        (&[0xf3, 0xf6, 0xc0, 0xff][..], opcode),
        (&[0xf0, 0x84, 0xc0][..], DecodeError::InvalidEncoding),
        (
            &[0x64, 0x84, 0xc0][..],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
    ] {
        let engine = code(CODE, bytes, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn exact_fetch_spans_and_missing_operands_keep_fault_categories() {
    for bytes in [&[0x84, 0xe0][..], &[0xa8, 0xff], &[0xf6, 0xc7, 0x80]] {
        let pc = 0x2000 - bytes.len() as u32;
        let engine = code(pc, bytes, true);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_eq!(decoded.length() as usize, bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
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
                .is_code_current(decoded.code_snapshot())
        );
    }
    for prefix in [&[0x84][..], &[0xa8], &[0xf6, 0xc7]] {
        let pc = 0x2000 - prefix.len() as u32;
        let engine = code(pc, prefix, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(DecodeError::MemoryFault {
                pc: GuestAddress(pc),
                fault: MemoryFault {
                    address: GuestAddress(0x2000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped,
                },
                length: prefix.len() as u32 + 1,
            })
        );
    }
    let engine = code(u32::MAX - 1, &[0xa8, 0xff], true);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(u32::MAX - 1))
            .unwrap()
            .next_pc(),
        GuestAddress(0)
    );
}

#[test]
fn failed_test_preparation_preserves_both_published_owners() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for bytes in [
                &[0x90, 0x66, 0x84, 0x03][..],
                &[0x90, 0x66, 0xf6, 0x03, 0xff],
                &[0x90, 0xf6, 0xc8, 0xff],
            ] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, bytes);
                describe(&mut engine, CODE, bytes.len(), entries);
                let before = publication(&engine, keep);
                assert_eq!(
                    compile(&mut engine, owner, entries),
                    Err(compile_error(
                        owner,
                        CODE + 1,
                        InstructionError::Decode(opcode)
                    ))
                );
                assert_eq!(publication(&engine, keep), before);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(CODE).is_err());
            }
            let (mut engine, keep) = prior_owners();
            upload(&mut engine, 0x1ffe, &[0xf6, 0xc7]);
            describe(&mut engine, 0x1ffe, 3, entries);
            let before = publication(&engine, keep);
            assert_eq!(
                compile(&mut engine, owner, entries),
                Err(compile_error(
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
                    })
                ))
            );
            assert_eq!(publication(&engine, keep), before);
            engine.guard(KEY, before.generation).unwrap();
            engine.guard_resident(KEY, keep).unwrap();
        }
    }
}
