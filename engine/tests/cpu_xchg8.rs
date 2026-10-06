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
            ir::{ByteRegister, ByteValue, Condition, Location32, Operation, UnaryKind},
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, ResidentInstallation},
};

#[test]
fn byte_xchg_al_ah_admits_before_jump_with_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0x1234_5678_9abc_def0).unwrap();
    let bytes = [0x86, 0xe0, 0xeb, 0];
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(&bytes);
    engine.upload(0x1000, bytes.len() as u32).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x1000), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(0x5000), Access::Read)
            .is_err()
    );
    let request = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    request[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    request[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    engine
        .compile(1)
        .expect("register byte exchange must admit before a jump without reading data");
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0xc886_0000_f123_4567;
const ALIASES: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];
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
const WITNESSES: [u8; 3] = [0xc0, 0xe0, 0xcf];

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

fn writable_code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    let mut engine = EngineInstance::new(pages + 2, KEY).unwrap();
    engine.map(base, pages, 7).unwrap();
    upload(&mut engine, pc, bytes);
    engine
}

fn code(pc: u32, bytes: &[u8]) -> EngineInstance {
    let mut engine = writable_code(pc, bytes);
    let base = pc & !0xfff;
    let pages = (u64::from(pc - base) + bytes.len() as u64).div_ceil(4096) as u32;
    engine.protect(base, pages, 4).unwrap();
    engine
}

fn describe(engine: &mut EngineInstance, pc: u32, length: u32, entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..8].copy_from_slice(&if entries { 0 } else { length }.to_le_bytes());
}

fn compile(engine: &mut EngineInstance, owner: Owner, entries: bool) -> Result<u64, HostError> {
    match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(1).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(1, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(1).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(1, 0).map(|id| id.get()),
    }
}

fn exchange(modrm: u8) -> Operation {
    assert_eq!(modrm >> 6, 3);
    Operation::ExchangeByte {
        left: ALIASES[usize::from(modrm & 7)],
        right: ALIASES[usize::from((modrm >> 3) & 7)],
    }
}

fn assert_decode(engine: &EngineInstance, pc: u32, length: u8, next: u32, operation: Operation) {
    let memory = engine.memory().unwrap();
    let decoded = decode_one(memory, GuestAddress(pc)).unwrap();
    assert_eq!(
        (decoded.pc(), decoded.length(), decoded.next_pc()),
        (GuestAddress(pc), length, GuestAddress(next))
    );
    assert_eq!(decoded.operation(), &operation);
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

fn page(engine: &EngineInstance, base: u32) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let memory = engine.memory().unwrap();
    memory
        .read(GuestAddress(base), &mut bytes)
        .or_else(|_| memory.fetch(GuestAddress(base), &mut bytes))
        .unwrap();
    bytes
}

fn unmapped_data(engine: &EngineInstance) {
    for pc in [0, DATA] {
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(pc), Access::Read)
                .is_err()
        );
    }
}

fn assert_module(bytes: &[u8]) {
    assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
    assert!(bytes.len() <= CompileLimits::default().wasm_bytes);
}

#[derive(Debug, PartialEq, Eq)]
struct Inputs {
    arena: Vec<u8>,
    pointer: usize,
    authority: (bool, u64),
    pages: Vec<(u32, Vec<u8>)>,
    mapped: u32,
}

fn inputs(engine: &EngineInstance, bases: &[u32]) -> Inputs {
    assert_eq!(engine.arena().len(), 4236);
    Inputs {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        authority: (engine.is_open(), engine.key()),
        pages: bases
            .iter()
            .map(|&base| (base, page(engine, base)))
            .collect(),
        mapped: engine.memory().unwrap().mapped_pages(),
    }
}

fn instruction_error(pc: u32, error: DecodeError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause: InstructionError::Decode(error),
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

fn host_error(owner: Owner, error: CompileError) -> HostError {
    match owner {
        Owner::Replacement => HostError::Compile(error),
        Owner::Resident => HostError::Resident(RegistryError::Compile(error)),
    }
}

fn six_failures(
    engine: &mut EngineInstance,
    pc: u32,
    length: u32,
    error: DecodeError,
    explicit: CompileError,
    bases: &[u32],
) -> usize {
    let entry = instruction_error(pc, error);
    let before = inputs(engine, bases);
    let memory = engine.memory().unwrap();
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: length,
            }],
            CompileLimits::default(),
        )
        .err(),
        Some(explicit)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default()).err(),
        Some(entry)
    );
    assert_eq!(inputs(engine, bases), before);
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            describe(engine, pc, length, entries);
            let before = inputs(engine, bases);
            let generation = engine.generation();
            assert_eq!(
                compile(engine, owner, entries),
                Err(host_error(owner, if entries { entry } else { explicit }))
            );
            assert_eq!(inputs(engine, bases), before);
            assert_eq!(engine.generation(), generation);
            assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
            assert!(engine.lookup_resident(pc).is_err());
        }
    }
    6
}

fn rejected(bytes: &[u8], error: DecodeError) -> usize {
    let mut engine = code(CODE, bytes);
    unmapped_data(&engine);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
        Some(error),
        "{bytes:02x?}"
    );
    six_failures(
        &mut engine,
        CODE,
        bytes.len() as u32,
        error,
        instruction_error(CODE, error),
        &[CODE],
    )
}

#[test]
fn all_sixty_four_ordered_byte_alias_pairs_admit_two_banks_in_six_profiles() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65536)
    );
    let mut identities = 0;
    let mut pair_classes = [0; 3];
    let mut admissions = 0;
    for first in [0_usize, 32] {
        let mut bytes = Vec::new();
        for ordinal in first..first + 32 {
            bytes.extend([0x86, 0xc0 | ordinal as u8]);
        }
        bytes.extend([0xeb, 0]);
        assert_eq!(bytes.len(), 66);
        let engine = code(CODE, &bytes);
        unmapped_data(&engine);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(CODE), Access::Read)
                .is_err()
        );
        for index in 0..32 {
            let ordinal = first + index;
            let left = ordinal % 8;
            let right = ordinal / 8;
            let pc = CODE + index as u32 * 2;
            assert_eq!(
                &bytes[index * 2..index * 2 + 2],
                &[0x86, 0xc0 | ordinal as u8]
            );
            assert_decode(
                &engine,
                pc,
                2,
                pc + 2,
                Operation::ExchangeByte {
                    left: ALIASES[left],
                    right: ALIASES[right],
                },
            );
            pair_classes[if left == right {
                0
            } else if left % 4 == right % 4 {
                1
            } else {
                2
            }] += 1;
            identities += 1;
        }
        let memory = engine.memory().unwrap();
        let extent = compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 66,
            }],
            limits,
        )
        .unwrap();
        let entry = compile_entry_region(memory, &[GuestAddress(CODE)], limits).unwrap();
        for compiled in [&extent, &entry] {
            assert_eq!(
                (compiled.metadata().blocks, compiled.metadata().instructions),
                (1, 33)
            );
            assert_module(compiled.wasm_bytes(memory).unwrap());
            admissions += 1;
        }
        assert_eq!(extent.metadata(), entry.metadata());
        assert_eq!(
            extent.wasm_bytes(memory).unwrap(),
            entry.wasm_bytes(memory).unwrap()
        );
        for owner in [Owner::Replacement, Owner::Resident] {
            for entries in [false, true] {
                let mut engine = code(CODE, &bytes);
                describe(&mut engine, CODE, 66, entries);
                let before = inputs(&engine, &[CODE]);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 66)
                    .unwrap();
                let id = compile(&mut engine, owner, entries).unwrap();
                assert_eq!(inputs(&engine, &[CODE]), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                unmapped_data(&engine);
                match owner {
                    Owner::Replacement => {
                        engine.guard(KEY, id as u32).unwrap();
                        assert_module(engine.artifact_bytes().unwrap());
                    }
                    Owner::Resident => {
                        engine.guard_resident(KEY, id).unwrap();
                        assert_module(engine.resident_bytes(id).unwrap());
                        for offset in (0..=64).step_by(2) {
                            assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                        }
                        for offset in (1..=65).step_by(2) {
                            assert!(engine.lookup_resident(CODE + offset).is_err());
                        }
                        assert!(engine.lookup_resident(CODE + 66).is_err());
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        assert_eq!(engine.generation(), 0);
                    }
                }
                admissions += 1;
            }
        }
    }
    assert_eq!(identities, 64);
    assert_eq!(pair_classes, [8, 8, 48]);
    assert_eq!(admissions, 12);
}

fn canonical_memory(modrm: u8) -> Vec<u8> {
    let mode = modrm >> 6;
    assert!(mode < 3);
    let rm = modrm & 7;
    let mut bytes = vec![0x86, modrm];
    if rm == 4 {
        bytes.push(0x24);
    }
    match mode {
        0 if rm == 5 => bytes.extend([0x10, 0x40, 0, 0]),
        1 => bytes.push(0x80),
        2 => bytes.extend([0x78, 0x56, 0x34, 0x12]),
        _ => {}
    }
    bytes
}

#[test]
fn full_modrm_domain_strict_categories_and_supported_neighbors_are_exact() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut domain = [0; 4];
    let mut compiler_refusals = 0;
    let mut negative_bytes = std::collections::BTreeSet::new();
    for raw in 0_u16..=255 {
        let modrm = raw as u8;
        let mode = usize::from(modrm >> 6);
        domain[mode] += 1;
        if mode == 3 {
            // The preceding typed-bank test owns these exact 64 positive identities.
            assert_eq!(modrm, 0xc0 | (raw as u8 & 63));
            continue;
        }
        let bytes = canonical_memory(modrm);
        assert!(negative_bytes.insert(bytes.clone()));
        compiler_refusals += rejected(&bytes, opcode);
    }
    assert_eq!(domain, [64, 64, 64, 64]);
    let mut strict = Vec::new();
    let prefixes = [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ];
    for prefix in prefixes {
        let prefixed = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            segment
        } else {
            opcode
        };
        for alias in 0_u8..8 {
            strict.push((
                vec![prefix, 0x86, 0xc0 | (alias << 3) | alias],
                if prefix == 0xf0 {
                    DecodeError::InvalidEncoding
                } else {
                    prefixed
                },
            ));
        }
        for memory in [&[0x86, 0x03][..], &[0x86, 0x04, 0x24][..]] {
            let mut bytes = vec![prefix];
            bytes.extend_from_slice(memory);
            strict.push((bytes, prefixed));
        }
    }
    strict.extend([
        (vec![0x66, 0x87, 0xc8], opcode),
        (vec![0x66, 0x97], opcode),
        (vec![0x66, 0x0f, 0xc0, 0xc8], opcode),
        (vec![0x0f, 0xc0, 0x03], opcode),
        (vec![0x66, 0x0f, 0xb0, 0xc8], opcode),
        (vec![0x0f, 0xb0, 0x03], opcode),
        (vec![0x0f, 0x0b], opcode),
    ]);
    assert_eq!(strict.len(), 117);
    let mut categories = [0; 3];
    for (bytes, error) in strict {
        assert!(negative_bytes.insert(bytes.clone()));
        categories[match error {
            DecodeError::InvalidEncoding => 0,
            DecodeError::Unsupported(UnsupportedFeature::Segment) => 1,
            DecodeError::Unsupported(UnsupportedFeature::Opcode) => 2,
            _ => panic!("unexpected strict category"),
        }] += 1;
        compiler_refusals += rejected(&bytes, error);
    }
    assert_eq!(categories, [8, 60, 49]);
    assert_eq!(negative_bytes.len(), 309);
    assert_eq!(compiler_refusals, 1854);

    let mut neighbors = vec![(
        vec![0x87, 0xc8],
        Operation::Exchange {
            left: Register32::Eax,
            right: Register32::Ecx,
        },
    )];
    for (index, register) in REGISTERS.into_iter().enumerate().skip(1) {
        neighbors.push((
            vec![0x90 + index as u8],
            Operation::Exchange {
                left: register,
                right: Register32::Eax,
            },
        ));
    }
    neighbors.extend([
        (vec![0x90], Operation::Nop),
        (
            vec![0x88, 0xe0],
            Operation::MoveByte {
                destination: ByteRegister::Al,
                source: ByteValue::Register(ByteRegister::Ah),
            },
        ),
        (
            vec![0x8a, 0xe0],
            Operation::MoveByte {
                destination: ByteRegister::Ah,
                source: ByteValue::Register(ByteRegister::Al),
            },
        ),
        (
            vec![0x0f, 0x92, 0xc4],
            Operation::SetByte {
                condition: Condition::Below,
                destination: ByteRegister::Ah,
            },
        ),
        (
            vec![0x0f, 0x90, 0xc0],
            Operation::SetByte {
                condition: Condition::Overflow,
                destination: ByteRegister::Al,
            },
        ),
    ]);
    assert_eq!(neighbors.len(), 13);
    let mut observations = 0;
    for (bytes, operation) in neighbors {
        let engine = code(CODE, &bytes);
        assert_decode(
            &engine,
            CODE,
            bytes.len() as u8,
            CODE + bytes.len() as u32,
            operation,
        );
        observations += 1;
    }
    let adjacent = code(CODE, &[0x48, 0x86, 0xc8]);
    assert_decode(
        &adjacent,
        CODE,
        1,
        CODE + 1,
        Operation::Unary {
            kind: UnaryKind::Dec,
            destination: Location32::Register(Register32::Eax),
        },
    );
    assert_decode(
        &adjacent,
        CODE + 1,
        2,
        CODE + 3,
        Operation::ExchangeByte {
            left: ByteRegister::Al,
            right: ByteRegister::Cl,
        },
    );
    observations += 2;
    assert_eq!(observations, 15);
}

fn three_admissions(
    engine: &mut EngineInstance,
    pc: u32,
    length: u32,
    entries: bool,
    bases: &[u32],
) -> usize {
    let before = inputs(engine, bases);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(pc), length as usize)
        .unwrap();
    let memory = engine.memory().unwrap();
    let compiled = if entries {
        compile_entry_region(memory, &[GuestAddress(pc)], CompileLimits::default())
    } else {
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: length,
            }],
            CompileLimits::default(),
        )
    }
    .unwrap();
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, 1)
    );
    assert_module(compiled.wasm_bytes(memory).unwrap());
    assert_eq!(inputs(engine, bases), before);
    for owner in [Owner::Replacement, Owner::Resident] {
        describe(engine, pc, length, entries);
        let before = inputs(engine, bases);
        let id = compile(engine, owner, entries).unwrap();
        assert_eq!(inputs(engine, bases), before);
        match owner {
            Owner::Replacement => {
                engine.guard(KEY, id as u32).unwrap();
                assert_module(engine.artifact_bytes().unwrap());
            }
            Owner::Resident => {
                engine.guard_resident(KEY, id).unwrap();
                assert_module(engine.resident_bytes(id).unwrap());
                assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
                assert!(engine.lookup_resident(pc.wrapping_add(1)).is_err());
                assert!(engine.lookup_resident(pc.wrapping_add(length)).is_err());
            }
        }
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
    3
}

#[test]
fn exact_two_byte_fetch_cuts_permissions_and_top_extent_are_preserved() {
    let mut complete = 0;
    let mut explicit_admissions = 0;
    let mut top_entry_admissions = 0;
    let mut cuts = 0;
    let mut permissions = 0;
    let mut compiler_errors = 0;
    for modrm in WITNESSES {
        let bytes = [0x86, modrm];
        for pc in [0x1ffe, 0x1fff, u32::MAX - 1] {
            let mut engine = code(pc, &bytes);
            assert_decode(&engine, pc, 2, pc.wrapping_add(2), exchange(modrm));
            unmapped_data(&engine);
            let bases = if pc == 0x1fff {
                vec![CODE, 0x2000]
            } else {
                vec![pc & !0xfff]
            };
            for &base in &bases {
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(base), Access::Read)
                        .is_err()
                );
            }
            if pc != 0x1fff {
                let following = if pc == 0x1ffe { 0x2000 } else { 0 };
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(following), Access::Execute)
                        .is_err()
                );
            }
            explicit_admissions += three_admissions(&mut engine, pc, 2, false, &bases);
            if pc == u32::MAX - 1 {
                let mut entry_engine = code(pc, &bytes);
                top_entry_admissions += three_admissions(&mut entry_engine, pc, 2, true, &bases);
            }
            complete += 1;
        }
        for pc in [0x1fff, u32::MAX] {
            let mut engine = code(pc, &[0x86]);
            let top = pc == u32::MAX;
            let error = fetch_error(
                pc,
                if top { pc } else { 0x2000 },
                2,
                if top {
                    FaultReason::AddressOverflow
                } else {
                    FaultReason::Unmapped
                },
            );
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(error)
            );
            compiler_errors += six_failures(
                &mut engine,
                pc,
                2,
                error,
                if top {
                    CompileError::InvalidBlocks
                } else {
                    instruction_error(pc, error)
                },
                &[pc & !0xfff],
            );
            cuts += 1;
        }
        let mut engine = code(0x1fff, &bytes);
        engine.protect(0x2000, 1, 3).unwrap();
        let error = fetch_error(0x1fff, 0x2000, 2, FaultReason::Permission);
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x1fff)).err(),
            Some(error)
        );
        compiler_errors += six_failures(
            &mut engine,
            0x1fff,
            2,
            error,
            instruction_error(0x1fff, error),
            &[CODE, 0x2000],
        );
        permissions += 1;
    }
    let mut opcode_controls = 0;
    for readable in [false, true] {
        let mut engine = code(CODE, &[0x86, 0xe0]);
        let pc = if readable { CODE } else { 0x2000 };
        if readable {
            engine.protect(CODE, 1, 1).unwrap();
        }
        let error = fetch_error(
            pc,
            pc,
            1,
            if readable {
                FaultReason::Permission
            } else {
                FaultReason::Unmapped
            },
        );
        assert_eq!(
            decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
            Some(error)
        );
        compiler_errors += six_failures(
            &mut engine,
            pc,
            2,
            error,
            instruction_error(pc, error),
            &[CODE],
        );
        opcode_controls += 1;
    }
    let mut poisoned = code(CODE, &[0x86, 0xe0, 0x0f, 0x0b]);
    assert_decode(&poisoned, CODE, 2, CODE + 2, exchange(0xe0));
    let consumed_span_admissions = three_admissions(&mut poisoned, CODE, 2, false, &[CODE]);
    assert_eq!(
        (complete, explicit_admissions, top_entry_admissions),
        (9, 27, 9)
    );
    assert_eq!(
        (cuts, permissions, opcode_controls, compiler_errors),
        (6, 3, 2, 66)
    );
    assert_eq!(consumed_span_admissions, 3);
}

#[test]
fn every_consumed_byte_write_stales_both_owners_and_fresh_ordered_ir_admits() {
    let pc = 0x1fff;
    let mut same_bytes = 0;
    let mut mutations = 0;
    for (old, offset, new, same) in WITNESSES
        .into_iter()
        .flat_map(|modrm| [(modrm, 0, 0x86, true), (modrm, 1, modrm, true)])
        .chain([
            (0xc0, 1, 0xe0, false),
            (0xe0, 1, 0xc4, false),
            (0xcf, 1, 0xf9, false),
        ])
    {
        let mut engine = writable_code(pc, &[0x86, old]);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_decode(&engine, pc, 2, pc + 2, exchange(old));
        describe(&mut engine, pc, 2, false);
        let generation = engine.compile(1).unwrap();
        let resident = engine.compile_resident(1).unwrap().get();
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        let before_pages = [page(&engine, CODE), page(&engine, 0x2000)];
        upload(&mut engine, pc + offset, &[new]);
        assert_eq!(
            engine.guard(KEY, generation),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.guard_resident(KEY, resident),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert!(
            !engine
                .memory()
                .unwrap()
                .is_code_current(decoded.code_snapshot())
        );
        let fresh = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        let expected = exchange(if offset == 0 { old } else { new });
        assert_eq!(fresh.operation(), &expected);
        assert_eq!(
            (fresh.pc(), fresh.length(), fresh.next_pc()),
            (GuestAddress(pc), 2, GuestAddress(pc + 2))
        );
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(fresh.code_snapshot())
        );
        if same {
            assert_eq!(fresh.operation(), decoded.operation());
            assert_eq!([page(&engine, CODE), page(&engine, 0x2000)], before_pages);
            same_bytes += 1;
        } else {
            assert_ne!(fresh.operation(), decoded.operation());
            let mut expected_pages = before_pages;
            expected_pages[1][0] = new;
            assert_eq!([page(&engine, CODE), page(&engine, 0x2000)], expected_pages);
            mutations += 1;
        }
        describe(&mut engine, pc, 2, false);
        let before = inputs(&engine, &[CODE, 0x2000]);
        let fresh_generation = engine.compile(1).unwrap();
        let fresh_resident = engine.compile_resident(1).unwrap().get();
        assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
        engine.guard(KEY, fresh_generation).unwrap();
        engine.guard_resident(KEY, fresh_resident).unwrap();
        assert!(
            engine
                .memory()
                .unwrap()
                .is_code_current(fresh.code_snapshot())
        );
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), fresh_resident);
        unmapped_data(&engine);
    }
    assert_eq!((same_bytes, mutations), (6, 3));
}

#[test]
fn unrelated_data_map_upload_protect_unmap_and_remap_preserve_code_and_owners() {
    let pc = 0x1fff;
    let mut scenarios = 0;
    for modrm in WITNESSES {
        let mut engine = code(pc, &[0x86, modrm]);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        describe(&mut engine, pc, 2, false);
        let generation = engine.compile(1).unwrap();
        let resident = engine.compile_resident(1).unwrap().get();
        let before_modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let before_pages = [page(&engine, CODE), page(&engine, 0x2000)];
        let pointer = engine.arena_address();
        let mut expected_arena = engine.arena().to_vec();
        unmapped_data(&engine);
        for stage in 0..7 {
            match stage {
                0 | 4 => engine.map(DATA, 1, 3).unwrap(),
                1 | 5 => {
                    let value = if stage == 1 { 0x80 } else { 0x81 };
                    expected_arena[TRANSFER_OFFSET] = value;
                    upload(&mut engine, DATA + 0x10, &[value]);
                }
                2 | 6 => engine.protect(DATA, 1, 1).unwrap(),
                3 => engine.unmap(DATA, 1).unwrap(),
                _ => unreachable!(),
            }
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            assert_eq!(engine.generation(), generation);
            assert_eq!((engine.is_open(), engine.key()), (true, KEY));
            assert_eq!(engine.arena_address(), pointer);
            assert_eq!(engine.arena(), expected_arena);
            assert_eq!(
                [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(resident).unwrap()
                ]
                .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
                before_modules
            );
            assert_eq!([page(&engine, CODE), page(&engine, 0x2000)], before_pages);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(decoded.code_snapshot())
            );
            assert_eq!(engine.lookup_resident(pc).unwrap().get(), resident);
            if stage == 3 {
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Read)
                        .is_err()
                );
            } else {
                assert_eq!(engine.memory().unwrap().mapped_pages(), 3);
                let mut expected = vec![0; 4096];
                if matches!(stage, 1 | 2 | 5 | 6) {
                    expected[0x10] = if stage < 4 { 0x80 } else { 0x81 };
                }
                assert_eq!(page(&engine, DATA), expected);
                assert_eq!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Write)
                        .is_ok(),
                    !matches!(stage, 2 | 6)
                );
            }
        }
        scenarios += 1;
    }
    assert_eq!(scenarios, 3);
}

#[derive(Debug, PartialEq, Eq)]
struct Publication {
    inputs: Inputs,
    generation: u32,
    entries: [u64; 2],
    modules: [(Vec<u8>, usize); 3],
    installed: ResidentInstallation,
}

fn publication(engine: &EngineInstance, resident: u64) -> Publication {
    Publication {
        inputs: inputs(engine, &[CODE, KEEP, DATA]),
        generation: engine.generation(),
        entries: [KEEP, KEEP + 1].map(|pc| engine.lookup_resident(pc).unwrap().get()),
        modules: [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(resident).unwrap(),
            engine.dispatcher_bytes(KEY).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        installed: engine.lookup_installed_resident(KEY, KEEP).unwrap(),
    }
}

#[test]
fn twenty_four_late_failures_preserve_complete_two_owner_publication_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut failures = 0;
    let mut prior_compiles = 0;
    for pair in [0xe0, 0xcf] {
        for (pc, bytes, length, expected) in [
            (
                CODE,
                vec![0x86, pair, 0x0f, 0x0b],
                4,
                instruction_error(CODE + 2, opcode),
            ),
            (
                CODE,
                vec![0x86, pair, 0x66, 0x86, pair],
                5,
                instruction_error(CODE + 2, opcode),
            ),
            (
                0x1ffd,
                vec![0x86, pair, 0x86],
                4,
                instruction_error(
                    0x1fff,
                    fetch_error(0x1fff, 0x2000, 2, FaultReason::Unmapped),
                ),
            ),
        ] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes);
                    assert_decode(&engine, pc, 2, pc + 2, exchange(pair));
                    engine.map(KEEP, 1, 7).unwrap();
                    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
                    engine.map(DATA, 1, 3).unwrap();
                    upload(&mut engine, DATA, &[0x5a; 4096]);
                    describe(&mut engine, KEEP, 3, false);
                    let generation = engine.compile(1).unwrap();
                    let resident = engine.compile_resident(1).unwrap().get();
                    prior_compiles += 2;
                    engine
                        .acknowledge_resident_installation(KEY, resident, 3)
                        .unwrap();
                    describe(&mut engine, pc, length, entries);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), 4096)
                        .unwrap();
                    let before = publication(&engine, resident);
                    assert_eq!(before.inputs.mapped, 3);
                    assert_eq!(before.inputs.authority, (true, KEY));
                    assert_eq!(before.entries, [resident; 2]);
                    assert_eq!(
                        before.installed,
                        ResidentInstallation {
                            unit_id: resident,
                            slot: 3
                        }
                    );
                    engine.guard(KEY, generation).unwrap();
                    engine.guard_resident(KEY, resident).unwrap();
                    assert_eq!(
                        compile(&mut engine, owner, entries),
                        Err(host_error(owner, expected))
                    );
                    assert_eq!(publication(&engine, resident), before);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    engine.guard(KEY, before.generation).unwrap();
                    engine.guard_resident(KEY, resident).unwrap();
                    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), resident);
                    assert_eq!(engine.lookup_resident(KEEP + 1).unwrap().get(), resident);
                    assert!(engine.lookup_resident(KEEP + 2).is_err());
                    for offset in 0..length {
                        assert!(engine.lookup_resident(pc + offset).is_err());
                    }
                    failures += 1;
                }
            }
        }
    }
    assert_eq!((failures, prior_compiles), (24, 48));
}
