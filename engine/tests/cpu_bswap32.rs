use ring3_engine::process::EngineInstance;

#[test]
fn bswap32_admits_before_jump() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let code = [0x0f, 0xc8, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(0x1000, 4).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[140..148];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..8].copy_from_slice(&4_u32.to_le_bytes());
    engine.compile(1).expect("bswap32 must admit before a jump");
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
            ir::{Location32, Operation, UnaryKind},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0xb587_0000_f123_4567;

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

fn code_page(engine: &EngineInstance) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(CODE), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn exact_eight_register_forms_admit_all_six_profiles() {
    let bytes = [
        0x0f, 0xc8, 0x0f, 0xc9, 0x0f, 0xca, 0x0f, 0xcb, 0x0f, 0xcc, 0x0f, 0xcd, 0x0f, 0xce, 0x0f,
        0xcf, 0xeb, 0,
    ];
    let engine = code(CODE, &bytes, true);
    let memory = engine.memory().unwrap();
    for (index, register) in [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ]
    .into_iter()
    .enumerate()
    {
        let pc = CODE + index as u32 * 2;
        let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ByteSwap {
                destination: register
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(pc + 2))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
    let mut admissions = 0;
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
            (1, 9)
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
            let page = code_page(&engine);
            let mapped = engine.memory().unwrap().mapped_pages();
            let id = compile(&mut engine, owner, entries).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(code_page(&engine), page);
            assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
            assert_eq!(mapped, 1);
            match owner {
                Owner::Replacement => {
                    engine.guard(KEY, id as u32).unwrap();
                    assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
                }
                Owner::Resident => {
                    engine.guard_resident(KEY, id).unwrap();
                    assert_eq!(&engine.resident_bytes(id).unwrap()[..8], b"\0asm\x01\0\0\0");
                    for offset in (0..=16).step_by(2) {
                        assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                    }
                    for offset in (1..=17).step_by(2) {
                        assert!(engine.lookup_resident(CODE + offset).is_err());
                    }
                    assert!(engine.lookup_resident(CODE + 18).is_err());
                    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                }
            }
            admissions += 1;
        }
    }
    assert_eq!(admissions, 6);
}

#[test]
fn strict_prefixes_fetch_wrap_and_fixed32_adjacency_keep_exact_categories() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut refusals = 0;
    for (prefix, error) in [
        (0x66, opcode),
        (0x67, opcode),
        (0xf2, opcode),
        (0xf3, opcode),
        (0x26, segment),
        (0x2e, segment),
        (0x36, segment),
        (0x3e, segment),
        (0x64, segment),
        (0x65, segment),
        (0xf0, DecodeError::InvalidEncoding),
    ] {
        let program = [prefix, 0x0f, 0xc8, 0xeb, 0];
        let engine = code(CODE, &program, true);
        let memory = engine.memory().unwrap();
        assert_eq!(decode_one(memory, GuestAddress(CODE)).err(), Some(error));
        let expected = Some(instruction_error(CODE, InstructionError::Decode(error)));
        assert_eq!(
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: program.len() as u32,
                }],
                CompileLimits::default(),
            )
            .err(),
            expected
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            expected
        );
        refusals += 1;
    }
    assert_eq!(refusals, 11);
    let adjacent = code(CODE, &[0x48, 0x0f, 0xc8], true);
    let first = decode_one(adjacent.memory().unwrap(), GuestAddress(CODE)).unwrap();
    assert_eq!(
        first.operation(),
        &Operation::Unary {
            kind: UnaryKind::Dec,
            destination: Location32::Register(Register32::Eax),
        }
    );
    assert_eq!(
        (first.length(), first.next_pc()),
        (1, GuestAddress(CODE + 1))
    );
    let second = decode_one(adjacent.memory().unwrap(), first.next_pc()).unwrap();
    assert_eq!(
        second.operation(),
        &Operation::ByteSwap {
            destination: Register32::Eax,
        }
    );
    assert_eq!(
        (second.length(), second.next_pc()),
        (2, GuestAddress(CODE + 3))
    );
    assert!(
        adjacent
            .memory()
            .unwrap()
            .is_code_current(second.code_snapshot())
    );
    for (pc, bytes, next) in [
        (CODE, &[0x0f, 0xc8, 0x0f, 0x0b][..], CODE + 2),
        (0x1ffe, &[0x0f, 0xc8][..], 0x2000),
        (0x1fff, &[0x0f, 0xc8][..], 0x2001),
        (u32::MAX - 1, &[0x0f, 0xc8][..], 0),
    ] {
        let engine = code(pc, bytes, true);
        let memory = engine.memory().unwrap();
        let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::ByteSwap {
                destination: Register32::Eax,
            }
        );
        assert_eq!(
            (decoded.length(), decoded.next_pc()),
            (2, GuestAddress(next))
        );
        assert!(memory.is_code_current(decoded.code_snapshot()));
        if pc == u32::MAX - 1 {
            let extent = compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 2,
                }],
                CompileLimits::default(),
            )
            .unwrap();
            let entry = compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
                .unwrap();
            for compiled in [&extent, &entry] {
                assert_eq!(
                    (compiled.metadata().blocks, compiled.metadata().instructions),
                    (1, 1)
                );
                assert_eq!(
                    &compiled.wasm_bytes(memory).unwrap()[..8],
                    b"\0asm\x01\0\0\0"
                );
            }
            assert_eq!(extent.metadata(), entry.metadata());
            assert_eq!(
                extent.wasm_bytes(memory).unwrap(),
                entry.wasm_bytes(memory).unwrap()
            );
        }
    }
    let mut permission = code(CODE, &[0x0f, 0xc8], false);
    permission.protect(CODE, 1, 1).unwrap();
    let unmapped = code(CODE, &[0x0f, 0xc8], true);
    let opcode_end = code(0x1fff, &[0x0f], true);
    let opcode_top = code(u32::MAX, &[0x0f], true);
    let mut second_permission = code(0x1fff, &[0x0f, 0xc8], false);
    second_permission.protect(0x2000, 1, 1).unwrap();
    for (engine, pc, expected) in [
        (
            unmapped,
            0x2000,
            fetch_error(0x2000, 0x2000, 1, FaultReason::Unmapped),
        ),
        (
            permission,
            CODE,
            fetch_error(CODE, CODE, 1, FaultReason::Permission),
        ),
        (
            opcode_end,
            0x1fff,
            fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
        ),
        (
            opcode_top,
            u32::MAX,
            fetch_error(u32::MAX, u32::MAX, 2, FaultReason::AddressOverflow),
        ),
        (
            second_permission,
            0x1fff,
            fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission),
        ),
    ] {
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(expected)
        );
    }
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
fn late_decode_failures_preserve_both_publications_and_all_mapped_pages() {
    let mut refusals = 0;
    for (pc, bytes, length, error_pc, error) in [
        (
            CODE,
            &[0x0f, 0xc8, 0x99][..],
            3,
            CODE + 2,
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            CODE,
            &[0x0f, 0xc8, 0x64, 0x0f, 0xc8][..],
            5,
            CODE + 2,
            DecodeError::Unsupported(UnsupportedFeature::Segment),
        ),
        (
            0x1ffd,
            &[0x0f, 0xc8, 0x0f][..],
            4,
            0x1fff,
            fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
        ),
    ] {
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, pc, bytes);
                describe(&mut engine, pc, length, entries);
                let before = publication(&engine, keep);
                let compile_error = instruction_error(error_pc, InstructionError::Decode(error));
                let expected = match owner {
                    Owner::Replacement => HostError::Compile(compile_error),
                    Owner::Resident => HostError::Resident(RegistryError::Compile(compile_error)),
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
