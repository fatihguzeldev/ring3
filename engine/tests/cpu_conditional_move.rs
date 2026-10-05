use ring3_engine::process::EngineInstance;

#[test]
fn register_conditional_move_admits_before_jump() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0x44, 0xc1, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..145].copy_from_slice(&code);
    engine.upload(0x1000, 5).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&5_u32.to_le_bytes());
    engine
        .compile(1)
        .expect("register conditional move must admit before a jump");
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
            Register32,
            decode::{DecodeError, decode_one},
            ir::{Condition, Operation},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x8abc_def0_f123_4567;
const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
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

#[derive(Clone, Copy)]
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
    let mut engine = EngineInstance::new(pages + 1, KEY).unwrap();
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

fn instruction_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
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

#[test]
fn every_condition_destination_and_source_has_exact_ir() {
    let mut engine = code(CODE, &[0x90], false);
    let (mut encodings, mut aliases) = (0, 0);
    for (cc, condition) in CONDITIONS.into_iter().enumerate() {
        for (destination_index, destination) in REGISTERS.into_iter().enumerate() {
            for (source_index, source) in REGISTERS.into_iter().enumerate() {
                let bytes = [
                    0x0f,
                    0x40 + cc as u8,
                    0xc0 | (destination_index as u8) << 3 | source_index as u8,
                ];
                upload(&mut engine, CODE, &bytes);
                let decoded = decode_one(engine.memory().unwrap(), GuestAddress(CODE)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::ConditionalMove {
                        condition,
                        destination,
                        source
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
                aliases += usize::from(source == destination);
            }
        }
    }
    assert_eq!((encodings, aliases), (1024, 128));
}

#[test]
fn conditions_registers_and_aliases_admit_all_six_extent_and_entry_profiles() {
    let (mut bytes, mut destinations, mut sources, mut aliases) = (Vec::new(), 0_u16, 0_u16, 0);
    for cc in 0..16_u8 {
        let destination = cc & 7;
        let source = if cc < 8 {
            (destination + 1) & 7
        } else {
            destination
        };
        bytes.extend_from_slice(&[0x0f, 0x40 + cc, 0xc0 | destination << 3 | source]);
        destinations |= 1 << destination;
        sources |= 1 << source;
        aliases += usize::from(destination == source);
    }
    assert_eq!(
        (bytes.len(), destinations, sources, aliases),
        (48, 0xff, 0xff, 8)
    );
    bytes.extend_from_slice(&[0xeb, 0]);
    let mut admissions = 0;
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
            (1, 17)
        );
        assert_eq!(
            &compiled.wasm_bytes(memory).unwrap()[..8],
            b"\0asm\x01\0\0\0"
        );
        admissions += 1;
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
                    assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                    for index in 0..=16 {
                        assert_eq!(engine.lookup_resident(CODE + index * 3).unwrap().get(), id);
                    }
                    assert!(engine.lookup_resident(CODE + 1).is_err());
                    assert_eq!(engine.generation(), 0);
                    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                }
            }
            admissions += 1;
        }
    }
    assert_eq!(admissions, 6);
}

#[test]
fn memory_sources_prefixes_and_exact_fetch_boundaries_keep_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut excluded: Vec<(Vec<u8>, DecodeError)> = (0..16_u8)
        .map(|cc| (vec![0x0f, 0x40 + cc, 0x00], opcode))
        .collect();
    for address in [
        &[0x04, 0x24][..],
        &[0x41, 0x80],
        &[0x44, 0x8b, 0x7f],
        &[0x83, 1, 2, 3, 4],
        &[0x84, 0x94, 1, 2, 3, 4],
        &[0x05, 0, 0x30, 0, 0],
        &[0x04, 0x95, 0, 0x30, 0, 0],
    ] {
        let mut bytes = vec![0x0f, 0x44];
        bytes.extend_from_slice(address);
        excluded.push((bytes, opcode));
    }
    for prefix in [0x66, 0x67, 0xf2, 0xf3] {
        excluded.push((vec![prefix, 0x0f, 0x44, 0xc1], opcode));
    }
    for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65] {
        excluded.push((
            vec![prefix, 0x0f, 0x44, 0xc1],
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ));
    }
    excluded.push((vec![0xf0, 0x0f, 0x44, 0xc1], DecodeError::InvalidEncoding));
    assert_eq!(excluded.len(), 34);
    let mut refused = 0;
    for (bytes, expected) in excluded {
        let mut engine = code(CODE, &bytes, true);
        engine.arena_mut().unwrap()[52..56].copy_from_slice(&2_u32.to_le_bytes());
        let memory = engine.memory().unwrap();
        assert_eq!(
            decode_one(memory, GuestAddress(CODE)).err(),
            Some(expected),
            "{bytes:02x?}"
        );
        let error = instruction_error(CODE, InstructionError::Decode(expected));
        assert_eq!(
            compile_region(
                memory,
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
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(error)
        );
        refused += 1;
    }
    assert_eq!(refused, 34);
    let mut fetched = 0;
    for bytes in [[0x0f, 0x40, 0xc1], [0x0f, 0x44, 0xed], [0x0f, 0x4f, 0xf7]] {
        for pc in [0x1ffd, 0x1ffe] {
            let engine = code(pc, &bytes, true);
            let memory = engine.memory().unwrap();
            let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
            assert_eq!(
                (decoded.length(), decoded.next_pc()),
                (3, GuestAddress(pc + 3))
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            if pc == 0x1ffd {
                assert!(
                    memory
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
            fetched += 1;
        }
    }
    let pc = u32::MAX - 2;
    let engine = code(pc, &[0x0f, 0x44, 0xc1], true);
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
    assert_eq!((decoded.length(), decoded.next_pc()), (3, GuestAddress(0)));
    assert!(memory.is_code_current(decoded.code_snapshot()));
    let compiled = compile_region(
        memory,
        &[BlockSpec {
            entry: GuestAddress(pc),
            byte_length: 3,
        }],
        CompileLimits::default(),
    )
    .unwrap();
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, 1)
    );
    fetched += 1;
    assert_eq!(fetched, 7);
    let mut failed_fetches = 0;
    for prefix in [&[0x0f][..], &[0x0f, 0x44]] {
        let pc = 0x2000 - prefix.len() as u32;
        let engine = code(pc, prefix, true);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(fetch_error(
                pc,
                0x2000,
                prefix.len() as u32 + 1,
                FaultReason::Unmapped
            ))
        );
        failed_fetches += 1;
    }
    let pc = u32::MAX - 1;
    let engine = code(pc, &[0x0f, 0x44], true);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
        Some(fetch_error(pc, pc, 3, FaultReason::AddressOverflow))
    );
    failed_fetches += 1;
    for (pc, page, fault_address, length) in [(CODE, CODE, CODE, 1), (0x1ffe, 0x2000, 0x2000, 3)] {
        let mut engine = code(pc, &[0x0f, 0x44, 0xc1], true);
        engine.protect(page, 1, 3).unwrap();
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(fetch_error(
                pc,
                fault_address,
                length,
                FaultReason::Permission
            ))
        );
        failed_fetches += 1;
    }
    assert_eq!(failed_fetches, 5);
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
    pages: [Vec<u8>; 3],
    mapped: u32,
}

fn publication(engine: &EngineInstance, id: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(id).unwrap(),
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
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for (pc, permissions) in [(CODE, 7), (KEEP, 7), (DATA, 3)] {
        engine.map(pc, 1, permissions).unwrap();
    }
    upload(&mut engine, DATA, &[0x5a; 4096]);
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

#[test]
fn late_refusals_preserve_both_publications_and_all_pages() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut refusals = 0;
    for (pc, bytes, declared_length, error_pc, expected) in [
        (
            CODE,
            &[0x0f, 0x44, 0xc1, 0x0f, 0x44, 0x00][..],
            6,
            CODE + 3,
            opcode,
        ),
        (
            CODE,
            &[0x0f, 0x44, 0xc1, 0x64, 0x0f, 0x44, 0xc1][..],
            7,
            CODE + 3,
            segment,
        ),
        (
            0x1ffb,
            &[0x0f, 0x44, 0xc1, 0x0f, 0x44][..],
            6,
            0x1ffe,
            fetch_error(0x1ffe, 0x2000, 3, FaultReason::Unmapped),
        ),
    ] {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, pc, bytes);
                describe(&mut engine, pc, declared_length, entries);
                engine.arena_mut().unwrap()[52..56].copy_from_slice(&2_u32.to_le_bytes());
                let before = publication(&engine, keep);
                let error = instruction_error(error_pc, InstructionError::Decode(expected));
                let expected = match owner {
                    Owner::Replacement => HostError::Compile(error),
                    Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
                };
                assert_eq!(compile(&mut engine, owner, entries), Err(expected));
                assert_eq!(publication(&engine, keep), before);
                engine.guard(KEY, before.generation).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
                assert!(engine.lookup_resident(pc).is_err());
                refusals += 1;
            }
        }
    }
    assert_eq!(refusals, 12);
}
