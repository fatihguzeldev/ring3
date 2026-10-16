use super::support_byte_prefix_admission as byte_prefix_admission;

use ring3_engine::{abi::arena::TRANSFER_OFFSET, process::EngineInstance};

fn admits_register_byte_accumulator_multiply(modrm: u8) {
    let bytes = [0xf6, modrm, 0xeb, 0x00];
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("register byte accumulator multiply must admit via public API");
}

#[test]
fn register_mul_byte_ah_admits_public_api() {
    admits_register_byte_accumulator_multiply(0xe4);
}

#[test]
fn register_imul_byte_ah_admits_public_api() {
    admits_register_byte_accumulator_multiply(0xec);
}

use ring3_engine::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_SIZE},
        memory_helper::HELPER_SIZE,
        x86::{EFLAGS_OFFSET, EIP_OFFSET, REGISTERS_OFFSET},
    },
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            compile_entry_region, compile_region,
        },
        x86::{
            decode::{DecodeError, decode_one},
            ir::{ByteRegister, MultiplyKind, Operation},
        },
    },
    memory::{Access, AddressSpace, FaultReason, GuestAddress, MemoryFault},
    process::{HostError, StoreCompletion},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0x1234_5678_9abc_def0;
const SOURCES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
const FORMS: [(MultiplyKind, u8); 2] =
    [(MultiplyKind::Unsigned, 0xe0), (MultiplyKind::Signed, 0xe8)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Replacement,
    Resident,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn fresh(pc: u32, bytes: &[u8], execute_only: bool) -> EngineInstance {
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
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
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

fn host_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn error(pc: u32, decode: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(decode),
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

fn assert_decoded(
    engine: &EngineInstance,
    pc: u32,
    kind: MultiplyKind,
    source: ByteRegister,
    modrm: u8,
) {
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::ByteMultiplyAccumulator { kind, source }
    );
    assert_eq!(
        (decoded.pc(), decoded.length(), decoded.next_pc()),
        (GuestAddress(pc), 2, GuestAddress(pc.wrapping_add(2)))
    );
    assert!(memory.is_code_current(decoded.code_snapshot()));
    let mut fetched = [0; 2];
    memory.fetch(GuestAddress(pc), &mut fetched).unwrap();
    assert_eq!(fetched, [0xf6, modrm]);
}

fn page(memory: &AddressSpace, pc: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    memory.read(GuestAddress(pc), &mut bytes).unwrap();
    bytes
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: [(Vec<u8>, usize); 3],
    pages: [Vec<u8>; 3],
    mapped: u32,
}

fn publication(engine: &EngineInstance, keep: u64) -> Publication {
    Publication {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        pages: [CODE, KEEP, DATA].map(|pc| page(engine.memory().unwrap(), pc)),
        mapped: engine.memory().unwrap().mapped_pages(),
    }
}

fn prior_owners() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for pc in [CODE, KEEP, DATA] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, KEEP, 3, false);
    engine.compile(1).unwrap();
    let keep = engine.compile_resident(1).unwrap().get();
    (engine, keep)
}

fn retained(engine: &EngineInstance, keep: u64, before: &Publication) {
    assert_eq!(publication(engine, keep), *before);
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, keep).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert!(engine.lookup_resident(CODE).is_err());
}

fn refused(
    engine: &mut EngineInstance,
    keep: u64,
    owner: Owner,
    entries: bool,
    pc: u32,
    length: usize,
    expected: CompileError,
) {
    describe(engine, pc, length, entries);
    if entries
        && crate::support_admission::admit_if_deferred(
            engine,
            crate::support_admission::ColdEntryCase {
                key: KEY,
                keeper: keep,
                resident: matches!(owner, Owner::Resident),
                pages: &[CODE, KEEP, DATA],
                expected,
            },
        )
    {
        return;
    }
    let before = publication(engine, keep);
    let memory = engine.memory().unwrap();
    let snapshots = [CODE, KEEP].map(|pc| memory.snapshot_code(GuestAddress(pc), 4096).unwrap());
    crate::support_fetch::assert_host_contract(
        (compile(engine, owner, entries)).expect_err("compilation must refuse this fixture"),
        host_error(owner, expected),
    );
    retained(engine, keep, &before);
    for snapshot in snapshots {
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
    assert!(engine.lookup_resident(pc).is_err());
}

fn validate_modules(modules: &[(u8, Vec<u8>)]) {
    assert_eq!(modules.len(), 6);
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
fn all_register_byte_accumulator_forms_have_exact_ir_and_six_profile_modules() {
    let mut bytes = Vec::new();
    for (_, base) in FORMS {
        for index in 0..8 {
            bytes.extend([0xf6, base | index]);
        }
    }
    bytes.extend([0xeb, 0]);
    assert_eq!(bytes.len(), 34);
    let engine = fresh(CODE, &bytes, true);
    let before = engine.arena().to_vec();
    let mut identities = 0;
    for (group, (kind, base)) in FORMS.into_iter().enumerate() {
        for (index, source) in SOURCES.into_iter().enumerate() {
            assert_decoded(
                &engine,
                CODE + (group * 16 + index * 2) as u32,
                kind,
                source,
                base | index as u8,
            );
            identities += 1;
        }
    }
    assert_eq!(identities, 16);
    let memory = engine.memory().unwrap();
    let mut modules = Vec::new();
    for region in [
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 34,
            }],
            CompileLimits::default(),
        )
        .unwrap(),
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).unwrap(),
    ] {
        assert_eq!(
            (region.metadata().blocks, region.metadata().instructions),
            (1, 17)
        );
        modules.push((0, region.wasm_bytes(memory).unwrap().to_vec()));
    }
    assert_eq!(engine.arena(), before);
    assert_eq!(memory.mapped_pages(), 1);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            let mut engine = fresh(CODE, &bytes, true);
            describe(&mut engine, CODE, bytes.len(), entries);
            let before = engine.arena().to_vec();
            let id = compile(&mut engine, owner, entries).unwrap();
            guard(&engine, owner, id).unwrap();
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
            assert_eq!(engine.generation(), u32::from(owner == Owner::Replacement));
            if owner == Owner::Resident {
                for offset in (0..34).step_by(2) {
                    assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                    assert!(engine.lookup_resident(CODE + offset + 1).is_err());
                }
                assert!(engine.lookup_resident(CODE + 34).is_err());
            }
            modules.push((
                if owner == Owner::Resident { 2 } else { 1 },
                module(&engine, owner, id).to_vec(),
            ));
        }
    }
    validate_modules(&modules);
}

fn strict_forms() -> Vec<(Vec<u8>, DecodeError)> {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut rows = Vec::new();
    for (_, base) in FORMS {
        for prefix in [
            0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
        ] {
            let expected = match prefix {
                0xf0 => DecodeError::InvalidEncoding,
                0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 => {
                    DecodeError::Unsupported(UnsupportedFeature::Segment)
                }
                _ => opcode,
            };
            rows.push((vec![prefix, 0xf6, base | 4], expected));
        }
        for tail in [
            &[0x20][..],
            &[0x60, 0x80],
            &[0xa0, 0x78, 0x56, 0x34, 0x12],
            &[0x25, 0x78, 0x56, 0x34, 0x12],
            &[0x24, 0x24],
            &[0x24, 0x8a],
            &[0x24, 0x8d, 0x78, 0x56, 0x34, 0x12],
        ] {
            let mut bytes = vec![0x66, 0xf6];
            bytes.extend_from_slice(tail);
            bytes[2] |= base & 8;
            rows.push((bytes, opcode));
        }
    }
    for bytes in [
        vec![0x66, 0x66, 0xf7, 0xe0],
        vec![0x66, 0x66, 0xf7, 0xe8],
        vec![0x66, 0xf6, 0xf0],
        vec![0x66, 0xf6, 0xf8],
        vec![0x66, 0x66, 0xf7, 0xf2],
        vec![0x66, 0x66, 0xf7, 0xfa],
    ] {
        rows.push((bytes, opcode));
    }
    assert_eq!(rows.len(), 42);
    assert_eq!(
        rows.iter().filter(|(_, error)| *error == opcode).count(),
        28
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, error)| *error == DecodeError::Unsupported(UnsupportedFeature::Segment))
            .count(),
        12
    );
    assert_eq!(
        rows.iter()
            .filter(|(_, error)| *error == DecodeError::InvalidEncoding)
            .count(),
        2
    );
    rows
}

#[test]
fn strict_neighbors_and_two_byte_fetch_keep_categories_and_boundaries() {
    let (mut direct, mut pure, mut bound) = (0, 0, 0);
    for (bytes, expected) in strict_forms() {
        let (bytes, expected) = byte_prefix_admission::closed_fixture(&bytes, expected);
        let engine = fresh(CODE, &bytes, true);
        let memory = engine.memory().unwrap();
        crate::support_fetch::assert_decode_contract(
            (decode_one(memory, GuestAddress(CODE))).expect_err("instruction must fail to decode"),
            expected,
        );
        direct += 1;
        for result in [
            compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            ),
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()),
        ] {
            assert_eq!(result.err(), Some(error(CODE, expected)), "{bytes:02x?}");
            pure += 1;
        }
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &bytes);
                refused(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    bytes.len(),
                    error(CODE, expected),
                );
                bound += 1;
            }
        }
    }
    assert_eq!((direct, pure, bound), (42, 84, 168));
    let mut complete = 0;
    for (kind, base) in FORMS {
        for pc in [0x1ffe, 0x1fff, 0xffff_fffe] {
            let modrm = base | 4;
            let engine = fresh(pc, &[0xf6, modrm], true);
            assert_decoded(&engine, pc, kind, ByteRegister::Ah, modrm);
            assert_eq!(
                engine.memory().unwrap().mapped_pages(),
                if pc == 0x1fff { 2 } else { 1 }
            );
            let region = compile_region(
                engine.memory().unwrap(),
                &[BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 2,
                }],
                CompileLimits::default(),
            )
            .unwrap();
            assert_eq!(
                (region.metadata().blocks, region.metadata().instructions),
                (1, 1)
            );
            assert_eq!(
                &region.wasm_bytes(engine.memory().unwrap()).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
            if pc == 0x1ffe {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(0x2000), Access::Execute)
                        .is_err()
                );
            }
            complete += 1;
        }
    }
    assert_eq!(complete, 6);
    let mut missing = 0;
    for (pc, address, reason) in [
        (0x1fff, 0x2000, FaultReason::Unmapped),
        (0x1fff, 0x2000, FaultReason::Permission),
        (0xffff_ffff, 0xffff_ffff, FaultReason::AddressOverflow),
    ] {
        let mut engine = fresh(pc, &[0xf6], true);
        if reason == FaultReason::Permission {
            engine.map(0x2000, 1, 3).unwrap();
        }
        crate::support_fetch::assert_decode_contract(
            (decode_one(engine.memory().unwrap(), GuestAddress(pc)))
                .expect_err("instruction must fail to decode"),
            fetch_error(pc, address, 2, reason),
        );
        missing += 1;
    }
    assert_eq!(missing, 3);
}

#[test]
fn late_byte_accumulator_decode_and_fetch_refusals_preserve_publications() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let (mut late_decode, mut late_fetch) = (0, 0);
    for (_, base) in FORMS {
        let modrm = base | 4;
        for (pc, bytes, declared, expected) in [
            (
                CODE,
                vec![0xf6, modrm, 0xf0, 0x90],
                4,
                error(CODE + 2, DecodeError::InvalidEncoding),
            ),
            (
                CODE,
                vec![0xf6, modrm, 0x66, 0x66, 0xf6, modrm ^ 8],
                6,
                error(CODE + 2, opcode),
            ),
            (
                0x1ffd,
                vec![0xf6, modrm, 0xf6],
                4,
                error(
                    0x1fff,
                    fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
                ),
            ),
        ] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, keep) = prior_owners();
                    upload(&mut engine, pc, &bytes);
                    refused(&mut engine, keep, owner, entries, pc, declared, expected);
                    if pc == CODE {
                        late_decode += 1;
                    } else {
                        late_fetch += 1;
                    }
                }
            }
        }
    }
    assert_eq!((late_decode, late_fetch), (16, 8));
}

struct Keep {
    id: u64,
    generation: u32,
    modules: [(Vec<u8>, usize); 2],
}

fn target_with_keep(base: u8, owner: Owner, entries: bool) -> (EngineInstance, u64, Keep) {
    let (mut engine, keep_id) = prior_owners();
    let keep = Keep {
        id: keep_id,
        generation: engine.generation(),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(keep_id).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
    };
    let bytes = [0xf6, base | 4, 0xeb, 0];
    upload(&mut engine, CODE, &bytes);
    describe(&mut engine, CODE, bytes.len(), entries);
    let target = compile(&mut engine, owner, entries).unwrap();
    (engine, target, keep)
}

fn keep_current(engine: &EngineInstance, owner: Owner, keep: &Keep) {
    engine.guard_resident(KEY, keep.id).unwrap();
    let bytes = engine.resident_bytes(keep.id).unwrap();
    assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &keep.modules[1]);
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep.id);
    if owner == Owner::Resident {
        engine.guard(KEY, keep.generation).unwrap();
        let bytes = engine.artifact_bytes().unwrap();
        assert_eq!(&(bytes.to_vec(), bytes.as_ptr() as usize), &keep.modules[0]);
    }
}

fn seed_cpu(engine: &mut EngineInstance) {
    for (index, value) in [
        0x1234_0000_u32,
        0x5566_7788,
        0x99aa_bbcc,
        0xddee_ff01,
        0x7000,
        0x8877_6655,
        0x4321_8765,
        0xabcdef12,
    ]
    .into_iter()
    .enumerate()
    {
        let at = REGISTERS_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine.arena_mut().unwrap()[EIP_OFFSET..EIP_OFFSET + 4].copy_from_slice(&CODE.to_le_bytes());
    engine.arena_mut().unwrap()[EFLAGS_OFFSET..EFLAGS_OFFSET + 4]
        .copy_from_slice(&0xcd7_u32.to_le_bytes());
}

fn stale_error(owner: Owner) -> HostError {
    match owner {
        Owner::Replacement => HostError::CodeInvalidated,
        Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
    }
}

fn check_store_helper(engine: &EngineInstance, before: &[u8]) {
    assert_eq!(HELPER_SIZE, 40);
    let words = [
        u32::from_le_bytes(*b"R3MH"),
        0x0001_0003,
        40,
        0,
        0,
        0,
        0,
        0,
        0,
        1,
    ];
    let mut expected = before.to_vec();
    for (index, word) in words.into_iter().enumerate() {
        let at = HELPER_OFFSET + index * 4;
        expected[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    assert_eq!(engine.arena(), expected);
}

fn owner_store(
    engine: &mut EngineInstance,
    owner: Owner,
    id: u64,
    pc: u32,
    value: u8,
) -> StoreCompletion {
    match owner {
        Owner::Replacement => engine.store8(pc, u32::from(value)),
        Owner::Resident => engine.store_resident8(KEY, id, pc, u32::from(value)),
    }
    .unwrap()
}

#[test]
fn byte_accumulator_inputs_currency_and_caps_keep_owner_state() {
    // native host observations only; generated multiply arithmetic and flags are not executed.
    let (mut inputs, mut data_writes, mut uploads, mut mutations, mut closed) = (0, 0, 0, 0, 0);
    for (_, base) in FORMS {
        let target = [0xf6, base | 4];
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, id, keep) = target_with_keep(base, owner, entries);
                seed_cpu(&mut engine);
                let bytes = module(&engine, owner, id).to_vec();
                let pointer = module(&engine, owner, id).as_ptr() as usize;
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 2)
                    .unwrap();
                for eax in [0x1234_0000_u32, 0x5678_00ff, 0x9abc_8001, 0xdef0_ff80] {
                    let mut expected = engine.arena().to_vec();
                    engine.arena_mut().unwrap()[REGISTERS_OFFSET..REGISTERS_OFFSET + 4]
                        .copy_from_slice(&eax.to_le_bytes());
                    expected[REGISTERS_OFFSET..REGISTERS_OFFSET + 4]
                        .copy_from_slice(&eax.to_le_bytes());
                    guard(&engine, owner, id).unwrap();
                    assert_eq!(engine.arena(), expected);
                    assert_eq!(
                        (
                            module(&engine, owner, id).to_vec(),
                            module(&engine, owner, id).as_ptr() as usize
                        ),
                        (bytes.clone(), pointer)
                    );
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    keep_current(&engine, owner, &keep);
                    inputs += 1;
                }
                let before = engine.arena().to_vec();
                engine.write8(DATA + 0x10, 0x81).unwrap();
                check_store_helper(&engine, &before);
                assert_eq!(page(engine.memory().unwrap(), DATA)[0x10], 0x81);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                guard(&engine, owner, id).unwrap();
                keep_current(&engine, owner, &keep);
                data_writes += 1;
                let before = engine.arena().to_vec();
                engine.close();
                crate::support_fetch::assert_host_contract(
                    (guard(&engine, owner, id)).expect_err("compilation must refuse this fixture"),
                    HostError::Closed,
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(engine.memory().err(), Some(HostError::Closed));
                closed += 1;

                let (mut engine, id, keep) = target_with_keep(base, owner, entries);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 2)
                    .unwrap();
                upload(&mut engine, CODE, &target);
                assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                let before = engine.arena().to_vec();
                crate::support_fetch::assert_host_contract(
                    (guard(&engine, owner, id)).expect_err("compilation must refuse this fixture"),
                    stale_error(owner),
                );
                match owner {
                    Owner::Replacement => {
                        assert_eq!(engine.artifact_bytes(), Err(stale_error(owner)))
                    }
                    Owner::Resident => {
                        assert_eq!(engine.resident_bytes(id), Err(stale_error(owner)))
                    }
                }
                assert_eq!(engine.arena(), before);
                keep_current(&engine, owner, &keep);
                uploads += 1;

                for (offset, original) in target.iter().copied().enumerate() {
                    for changing in [false, true] {
                        let (mut engine, id, keep) = target_with_keep(base, owner, entries);
                        let snapshot = engine
                            .memory()
                            .unwrap()
                            .snapshot_code(GuestAddress(CODE), 2)
                            .unwrap();
                        let generation = engine.generation();
                        let value = original ^ u8::from(changing);
                        let before = engine.arena().to_vec();
                        assert_eq!(
                            owner_store(&mut engine, owner, id, CODE + offset as u32, value),
                            StoreCompletion::CodeInvalidated
                        );
                        check_store_helper(&engine, &before);
                        assert!(!engine.memory().unwrap().is_code_current(&snapshot));
                        let mut fetched = [0];
                        engine
                            .memory()
                            .unwrap()
                            .fetch(GuestAddress(CODE + offset as u32), &mut fetched)
                            .unwrap();
                        assert_eq!(fetched, [value]);
                        let before = engine.arena().to_vec();
                        crate::support_fetch::assert_host_contract(
                            (guard(&engine, owner, id))
                                .expect_err("compilation must refuse this fixture"),
                            stale_error(owner),
                        );
                        match owner {
                            Owner::Replacement => {
                                assert_eq!(engine.artifact_bytes(), Err(stale_error(owner)))
                            }
                            Owner::Resident => {
                                assert_eq!(engine.resident_bytes(id), Err(stale_error(owner)))
                            }
                        }
                        assert_eq!(engine.generation(), generation);
                        assert_eq!(engine.arena(), before);
                        keep_current(&engine, owner, &keep);
                        mutations += 1;
                    }
                }
            }
        }
    }
    assert_eq!(
        (inputs, data_writes, uploads, mutations, closed),
        (32, 8, 8, 32, 8)
    );
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.blocks, limits.instructions, limits.wasm_bytes),
        (8, 64, 65_536)
    );
    let (mut admitted, mut rejected, mut custom_instruction, mut custom_wasm) = (0, 0, 0, 0);
    for (_, base) in FORMS {
        let mut positive = [0xf6, base | 4].repeat(63);
        positive.extend([0xeb, 0]);
        let mut negative = [0xf6, base | 4].repeat(64);
        negative.extend([0xeb, 0]);
        assert_eq!((positive.len(), negative.len()), (128, 130));
        for bytes in [&positive, &negative] {
            let engine = fresh(CODE, bytes, true);
            let memory = engine.memory().unwrap();
            for (entry, result) in [
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(CODE),
                        byte_length: bytes.len() as u32,
                    }],
                    limits,
                ),
                compile_entry_region(memory, &[GuestAddress(CODE)], limits),
            ]
            .into_iter()
            .enumerate()
            {
                if bytes.len() == positive.len() {
                    let region = result.unwrap();
                    assert_eq!(
                        (region.metadata().blocks, region.metadata().instructions),
                        (1, 64)
                    );
                    assert!(region.wasm_bytes(memory).unwrap().len() <= 65_536);
                    admitted += 1;
                } else {
                    if entry == 0 {
                        assert_eq!(result.err(), Some(CompileError::InstructionLimit));
                    } else {
                        let region = result.unwrap();
                        assert_eq!(region.metadata().instructions, 64);
                        assert!(region.wasm_bytes(memory).unwrap().len() <= 65_536);
                    }
                    rejected += 1;
                }
            }
        }
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &positive);
                describe(&mut engine, CODE, positive.len(), entries);
                let before = engine.arena().to_vec();
                let id = compile(&mut engine, owner, entries).unwrap();
                guard(&engine, owner, id).unwrap();
                engine.guard_resident(KEY, keep).unwrap();
                assert_eq!(engine.arena(), before);
                assert!(module(&engine, owner, id).len() <= 65_536);
                admitted += 1;
                let (mut engine, keep) = prior_owners();
                upload(&mut engine, CODE, &negative);
                refused(
                    &mut engine,
                    keep,
                    owner,
                    entries,
                    CODE,
                    negative.len(),
                    CompileError::InstructionLimit,
                );
                rejected += 1;
            }
        }
        let engine = fresh(CODE, &[0xf6, base | 4, 0xeb, 0], true);
        let memory = engine.memory().unwrap();
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
            for (entry, result) in [
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(CODE),
                        byte_length: 4,
                    }],
                    limits,
                ),
                compile_entry_region(memory, &[GuestAddress(CODE)], limits),
            ]
            .into_iter()
            .enumerate()
            {
                if entry == 1 && expected == CompileError::InstructionLimit {
                    let region = result.unwrap();
                    assert_eq!(region.metadata().instructions, 1);
                    assert!(region.wasm_bytes(memory).unwrap().len() <= 65_536);
                } else {
                    assert_eq!(result.err(), Some(expected));
                }
                match expected {
                    CompileError::InstructionLimit => custom_instruction += 1,
                    CompileError::WasmLimit => custom_wasm += 1,
                    _ => unreachable!(),
                }
            }
        }
    }
    assert_eq!(
        (admitted, rejected, custom_instruction, custom_wasm),
        (12, 12, 4, 4)
    );
}
