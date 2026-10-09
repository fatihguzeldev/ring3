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
            ir::{
                ByteRegister, ByteValue, EffectiveAddress, Location32, Operation, UnaryKind,
                Value32,
            },
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, ResidentInstallation},
};

#[test]
fn modrm_byte_move_ah_ff_admits_before_jump_with_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0xc600_0000_f123_4567).unwrap();
    let bytes = [0xc6, 0xc4, 0xff, 0xeb, 0];
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
        .expect("register C6 byte immediate move must admit before a jump without reading data");
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0xc600_0000_f123_4567;
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

fn describe(engine: &mut EngineInstance, rows: &[(u32, u32)], entries: bool) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    let stride = if entries { 4 } else { 8 };
    for (index, &(pc, length)) in rows.iter().enumerate() {
        let offset = index * stride;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        if !entries {
            transfer[offset + 4..offset + 8].copy_from_slice(&length.to_le_bytes());
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

fn move_byte(alias: usize, immediate: u8) -> Operation {
    Operation::MoveByte {
        destination: ALIASES[alias],
        source: ByteValue::Immediate(immediate),
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

fn byte(bytes: &mut &[u8]) -> u8 {
    let (&value, remaining) = bytes.split_first().unwrap();
    *bytes = remaining;
    value
}

fn unsigned(bytes: &mut &[u8]) -> u32 {
    let mut value = 0;
    for index in 0..5 {
        let next = byte(bytes);
        if index == 4 {
            assert_eq!(next & 0xf0, 0);
        }
        value |= u32::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            return value;
        }
    }
    panic!("unterminated unsigned LEB");
}

fn signed_word(bytes: &mut &[u8]) -> u32 {
    let mut value = 0_i64;
    for index in 0..5 {
        let next = byte(bytes);
        let bits = (index + 1) * 7;
        value |= i64::from(next & 0x7f) << (index * 7);
        if next & 0x80 == 0 {
            if next & 0x40 != 0 {
                value -= 1_i64 << bits;
            }
            assert!(i32::try_from(value).is_ok());
            return value as i32 as u32;
        }
    }
    panic!("unterminated signed LEB");
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> &'a [u8] {
    let (value, remaining) = bytes.split_at(length);
    *bytes = remaining;
    value
}

fn name<'a>(bytes: &mut &'a [u8]) -> &'a str {
    let length = unsigned(bytes) as usize;
    std::str::from_utf8(take(bytes, length)).unwrap()
}

fn section(wasm: &[u8], requested: u8) -> &[u8] {
    assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
    let mut bytes = &wasm[8..];
    while !bytes.is_empty() {
        let kind = byte(&mut bytes);
        let length = unsigned(&mut bytes) as usize;
        let value = take(&mut bytes, length);
        if kind == requested {
            return value;
        }
    }
    panic!("missing section {requested}");
}

fn assert_module(wasm: &[u8], owner: Option<Owner>, id: u64, store: bool) {
    assert!(wasm.len() <= CompileLimits::default().wasm_bytes);
    let resident = matches!(owner, Some(Owner::Resident));
    let mut arities = vec![4];
    let mut helpers = Vec::new();
    if owner.is_some() {
        arities.push(if resident { 7 } else { 6 });
        helpers.push((if resident { "guard_resident" } else { "guard" }, 1));
    }
    if store {
        assert!(owner.is_some());
        arities.push(if resident { 6 } else { 2 });
        helpers.push((
            if resident {
                "store_resident8"
            } else {
                "store8"
            },
            2,
        ));
    }
    let mut types = section(wasm, 1);
    assert_eq!(unsigned(&mut types) as usize, arities.len());
    for arity in arities {
        assert_eq!(byte(&mut types), 0x60);
        assert_eq!(unsigned(&mut types), arity);
        assert_eq!(take(&mut types, arity as usize), vec![0x7f; arity as usize]);
        assert_eq!((unsigned(&mut types), byte(&mut types)), (1, 0x7f));
    }
    assert!(types.is_empty());
    let mut imports = section(wasm, 2);
    assert_eq!(unsigned(&mut imports) as usize, helpers.len() + 1);
    assert_eq!((name(&mut imports), name(&mut imports)), ("env", "memory"));
    assert_eq!(byte(&mut imports), 2);
    assert_eq!((unsigned(&mut imports), unsigned(&mut imports)), (0, 1));
    for &(field, type_index) in &helpers {
        assert_eq!((name(&mut imports), name(&mut imports)), ("ring3", field));
        assert_eq!(
            (byte(&mut imports), unsigned(&mut imports)),
            (0, type_index)
        );
    }
    assert!(imports.is_empty());
    let mut functions = section(wasm, 3);
    assert_eq!((unsigned(&mut functions), unsigned(&mut functions)), (1, 0));
    assert!(functions.is_empty());
    let mut exports = section(wasm, 7);
    assert_eq!(unsigned(&mut exports), 1);
    assert_eq!(name(&mut exports), "run");
    assert_eq!(
        (byte(&mut exports), unsigned(&mut exports)),
        (0, helpers.len() as u32)
    );
    assert!(exports.is_empty());
    let mut bodies = section(wasm, 10);
    assert_eq!(unsigned(&mut bodies), 1);
    let length = unsigned(&mut bodies) as usize;
    let mut body = take(&mut bodies, length);
    assert!(bodies.is_empty());
    assert_eq!(unsigned(&mut body), if store { 3 } else { 2 });
    for expected in [(16, 0x7f), (1, 0x7e)] {
        assert_eq!((unsigned(&mut body), byte(&mut body)), expected);
    }
    if store {
        assert_eq!((unsigned(&mut body), byte(&mut body)), (6, 0x7f));
    }
    if owner.is_some() {
        let mut constants = vec![KEY as u32, (KEY >> 32) as u32, id as u32];
        if resident {
            constants.push((id >> 32) as u32);
        }
        for value in constants {
            assert_eq!(byte(&mut body), 0x41);
            assert_eq!(signed_word(&mut body), value);
        }
        for local in [0, 1, 3] {
            assert_eq!((byte(&mut body), unsigned(&mut body)), (0x20, local));
        }
        assert_eq!((byte(&mut body), unsigned(&mut body)), (0x10, 0));
    }
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
    assert_eq!(engine.arena().len(), ring3_engine::abi::arena::ARENA_SIZE);
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
                byte_length: length
            }],
            CompileLimits::default()
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
            describe(engine, &[(pc, length)], entries);
            let before = inputs(engine, bases);
            let generation = engine.generation();
            assert_eq!(
                compile(engine, owner, entries, 1),
                Err(host_error(owner, if entries { entry } else { explicit }))
            );
            assert_eq!(inputs(engine, bases), before);
            assert_eq!(engine.generation(), generation);
            assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
            for offset in 0..length {
                assert!(engine.lookup_resident(pc.wrapping_add(offset)).is_err());
            }
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

fn bank(alias: u8, first: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    for index in 0_u8..32 {
        bytes.extend([0xc6, 0xc0 | alias, first + index]);
    }
    bytes.extend([0xeb, 0]);
    assert_eq!(bytes.len(), 98);
    bytes
}

#[test]
fn every_alias_and_unsigned_immediate_admits_sixty_four_banks_in_six_profiles() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65536)
    );
    let mut identities = std::collections::BTreeSet::new();
    let mut banks = 0;
    let mut admissions = [0; 2];
    for alias in 0_u8..8 {
        for chunk in 0_u8..8 {
            let bytes = bank(alias, chunk * 32);
            let engine = code(CODE, &bytes);
            unmapped_data(&engine);
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(CODE), Access::Read)
                    .is_err()
            );
            for index in 0_u8..32 {
                let immediate = chunk * 32 + index;
                assert!(identities.insert((alias, immediate)));
                let pc = CODE + u32::from(index) * 3;
                assert_eq!(
                    &bytes[usize::from(index) * 3..usize::from(index) * 3 + 3],
                    &[0xc6, 0xc0 | alias, immediate]
                );
                assert_decode(
                    &engine,
                    pc,
                    3,
                    pc + 3,
                    move_byte(usize::from(alias), immediate),
                );
            }
            let before = inputs(&engine, &[CODE]);
            let memory = engine.memory().unwrap();
            let extent = compile_region(
                memory,
                &[BlockSpec {
                    entry: GuestAddress(CODE),
                    byte_length: 98,
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
                assert_module(compiled.wasm_bytes(memory).unwrap(), None, 0, false);
                admissions[0] += 1;
            }
            assert_eq!(extent.metadata(), entry.metadata());
            assert_eq!(
                extent.wasm_bytes(memory).unwrap(),
                entry.wasm_bytes(memory).unwrap()
            );
            assert_eq!(inputs(&engine, &[CODE]), before);
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &bytes);
                    describe(&mut engine, &[(CODE, 98)], entries);
                    let before = inputs(&engine, &[CODE]);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), 98)
                        .unwrap();
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(inputs(&engine, &[CODE]), before);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    guard(&engine, owner, id).unwrap();
                    assert_module(module(&engine, owner, id), Some(owner), id, false);
                    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
                    unmapped_data(&engine);
                    if matches!(owner, Owner::Resident) {
                        for index in 0..32 {
                            assert_eq!(engine.lookup_resident(CODE + index * 3).unwrap().get(), id);
                            for interior in [1, 2] {
                                assert!(
                                    engine.lookup_resident(CODE + index * 3 + interior).is_err()
                                );
                            }
                        }
                        assert_eq!(engine.lookup_resident(CODE + 96).unwrap().get(), id);
                        for offset in [97, 98] {
                            assert!(engine.lookup_resident(CODE + offset).is_err());
                        }
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        assert_eq!(engine.generation(), 0);
                    }
                    admissions[1] += 1;
                }
            }
            banks += 1;
        }
    }
    assert_eq!((identities.len(), banks), (2048, 64));
    assert_eq!(admissions, [128, 256]);
}

fn canonical_c6(modrm: u8) -> Vec<u8> {
    let mode = modrm >> 6;
    let rm = modrm & 7;
    let mut bytes = vec![0xc6, modrm];
    if mode != 3 {
        if rm == 4 {
            bytes.push(0x24);
        }
        match mode {
            0 if rm == 5 => bytes.extend([0x10, 0x40, 0, 0]),
            1 => bytes.push(0x80),
            2 => bytes.extend([0x78, 0x56, 0x34, 0x12]),
            _ => {}
        }
    }
    bytes.push(0x81);
    bytes
}

fn memory_store(modrm: u8) -> Operation {
    let mode = modrm >> 6;
    let rm = usize::from(modrm & 7);
    assert!(mode < 3);
    assert_eq!((modrm >> 3) & 7, 0);
    Operation::StoreByte {
        address: EffectiveAddress {
            base: if mode == 0 && rm == 5 {
                None
            } else {
                Some(REGISTERS[rm])
            },
            index: None,
            scale: 1,
            displacement: match mode {
                0 if rm == 5 => 0x4010,
                1 => 0xffff_ff80,
                2 => 0x1234_5678,
                _ => 0,
            },
        },
        source: ByteValue::Immediate(0x81),
    }
}

fn category(error: DecodeError) -> usize {
    match error {
        DecodeError::InvalidEncoding => 0,
        DecodeError::Unsupported(UnsupportedFeature::Segment) => 1,
        DecodeError::Unsupported(UnsupportedFeature::Opcode) => 2,
        _ => panic!("unexpected strict category"),
    }
}

#[test]
fn full_modrm_domain_strict_categories_and_supported_neighbors_are_exact() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let segment = DecodeError::Unsupported(UnsupportedFeature::Segment);
    let mut domain = [0; 4];
    let mut negative_bytes = std::collections::BTreeSet::new();
    let mut categories = [0; 3];
    let mut strict_compiler_errors = 0;
    let mut memory_admissions = 0;
    let mut memory_backend_errors = 0;
    for raw in 0_u16..=255 {
        let modrm = raw as u8;
        let bytes = canonical_c6(modrm);
        if (modrm >> 3) & 7 == 0 {
            if modrm >> 6 == 3 {
                // test 2 owns these eight exact destination/immediate81 identities.
                assert_eq!(bytes, [0xc6, 0xc0 | (modrm & 7), 0x81]);
                domain[0] += 1;
                continue;
            }
            domain[1] += 1;
            let length = bytes.len() as u32;
            let mut program = bytes.clone();
            program.extend([0xeb, 0]);
            let engine = code(CODE, &program);
            unmapped_data(&engine);
            assert_decode(
                &engine,
                CODE,
                length as u8,
                CODE + length,
                memory_store(modrm),
            );
            let expected = CompileError::Instruction {
                pc: GuestAddress(CODE),
                cause: InstructionError::BackendUnsupported,
            };
            let before = inputs(&engine, &[CODE]);
            let memory = engine.memory().unwrap();
            assert_eq!(
                compile_region(
                    memory,
                    &[BlockSpec {
                        entry: GuestAddress(CODE),
                        byte_length: program.len() as u32
                    }],
                    CompileLimits::default()
                )
                .err(),
                Some(expected)
            );
            assert_eq!(
                compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
                Some(expected)
            );
            assert_eq!(inputs(&engine, &[CODE]), before);
            memory_backend_errors += 2;
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(CODE, &program);
                    describe(&mut engine, &[(CODE, program.len() as u32)], entries);
                    let before = inputs(&engine, &[CODE]);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), program.len())
                        .unwrap();
                    let id = compile(&mut engine, owner, entries, 1).unwrap();
                    assert_eq!(inputs(&engine, &[CODE]), before);
                    guard(&engine, owner, id).unwrap();
                    assert_module(module(&engine, owner, id), Some(owner), id, true);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    unmapped_data(&engine);
                    if matches!(owner, Owner::Resident) {
                        for offset in [0, length] {
                            assert_eq!(engine.lookup_resident(CODE + offset).unwrap().get(), id);
                        }
                        for offset in 1..length {
                            assert!(engine.lookup_resident(CODE + offset).is_err());
                        }
                        for offset in [length + 1, length + 2] {
                            assert!(engine.lookup_resident(CODE + offset).is_err());
                        }
                        assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                        assert_eq!(engine.generation(), 0);
                    }
                    memory_admissions += 1;
                }
            }
        } else {
            let error = if modrm == 0xf8 {
                domain[2] += 1;
                opcode
            } else {
                domain[3] += 1;
                DecodeError::InvalidEncoding
            };
            assert!(negative_bytes.insert(bytes.clone()));
            categories[category(error)] += 1;
            strict_compiler_errors += rejected(&bytes, error);
        }
    }
    assert_eq!(domain, [8, 24, 1, 223]);
    assert_eq!((memory_admissions, memory_backend_errors), (96, 48));
    let mut strict = Vec::new();
    for prefix in [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ] {
        let error = if prefix == 0xf0 {
            DecodeError::InvalidEncoding
        } else if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            segment
        } else {
            opcode
        };
        for alias in 0_u8..8 {
            for immediate in [0, 0x80, 0xff] {
                strict.push((vec![prefix, 0xc6, 0xc0 | alias, immediate], error));
            }
        }
    }
    for register in 0_u8..8 {
        strict.push((vec![0x66, 0x66, 0xc7, 0xc0 | register, 0x34, 0x12], opcode));
    }
    for immediate in [0, 0x80, 0xff] {
        strict.push((vec![0xc6, 0xf8, immediate], opcode));
    }
    assert_eq!(strict.len(), 275);
    let mut additional_categories = [0; 3];
    for (bytes, error) in strict {
        assert!(negative_bytes.insert(bytes.clone()));
        categories[category(error)] += 1;
        additional_categories[category(error)] += 1;
        strict_compiler_errors += rejected(&bytes, error);
    }
    assert_eq!(additional_categories, [24, 144, 107]);
    assert_eq!(categories, [247, 144, 108]);
    assert_eq!(negative_bytes.len(), 499);
    assert_eq!(strict_compiler_errors, 2994);

    let mut neighbors = Vec::new();
    for alias in 0_u8..8 {
        for immediate in [0, 0x80, 0xff] {
            neighbors.push((
                vec![0xb0 + alias, immediate],
                move_byte(usize::from(alias), immediate),
            ));
        }
        let partner = alias ^ 4;
        for (opcode, modrm) in [
            (0x88, 0xc0 | (partner << 3) | alias),
            (0x8a, 0xc0 | (alias << 3) | partner),
        ] {
            neighbors.push((
                vec![opcode, modrm],
                Operation::MoveByte {
                    destination: ALIASES[usize::from(alias)],
                    source: ByteValue::Register(ALIASES[usize::from(partner)]),
                },
            ));
        }
        let operation = Operation::Move {
            destination: Location32::Register(REGISTERS[usize::from(alias)]),
            source: Value32::Immediate(0x1234_5678),
        };
        neighbors.push((vec![0xc7, 0xc0 | alias, 0x78, 0x56, 0x34, 0x12], operation));
        neighbors.push((vec![0xb8 + alias, 0x78, 0x56, 0x34, 0x12], operation));
    }
    assert_eq!(neighbors.len(), 56);
    let mut neighbor_bytes = std::collections::BTreeSet::new();
    let mut observations = 0;
    for (bytes, operation) in neighbors {
        assert!(neighbor_bytes.insert(bytes.clone()));
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
    for (prefix, alias, kind) in [(0x40, 0, UnaryKind::Inc), (0x48, 4, UnaryKind::Dec)] {
        let engine = code(CODE, &[prefix, 0xc6, 0xc0 | alias, 0x81]);
        assert_decode(
            &engine,
            CODE,
            1,
            CODE + 1,
            Operation::Unary {
                kind,
                destination: Location32::Register(Register32::Eax),
            },
        );
        assert_decode(
            &engine,
            CODE + 1,
            3,
            CODE + 4,
            move_byte(usize::from(alias), 0x81),
        );
        observations += 2;
    }
    assert_eq!(observations, 60);
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

fn prior_owners(mut engine: EngineInstance) -> (EngineInstance, u64) {
    engine.map(KEEP, 1, 7).unwrap();
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    engine.map(DATA, 1, 3).unwrap();
    let pattern: Vec<u8> = (0..4096).map(|index| (index % 251 + 1) as u8).collect();
    upload(&mut engine, DATA, &pattern);
    describe(&mut engine, &[(KEEP, 3)], false);
    engine.compile(1).unwrap();
    let resident = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, resident, 3)
        .unwrap();
    (engine, resident)
}

fn retained(engine: &EngineInstance, resident: u64, before: &Publication, failed: &[(u32, u32)]) {
    assert_eq!(publication(engine, resident), *before);
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
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, resident).unwrap();
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), resident);
    assert_eq!(engine.lookup_resident(KEEP + 1).unwrap().get(), resident);
    assert!(engine.lookup_resident(KEEP + 2).is_err());
    for &(pc, length) in failed {
        for offset in 0..length {
            assert!(engine.lookup_resident(pc + offset).is_err());
        }
        assert!(engine.lookup_resident(pc + length).is_err());
    }
}

#[test]
fn default_and_custom_caps_keep_full_installed_publication() {
    let ordinary = bank(0, 0);
    let nine_rows: Vec<(u32, u32)> = (0..9).map(|index| (CODE + index * 8, 5)).collect();
    let mut nine_source = vec![0; 69];
    for index in 0..9 {
        nine_source[index * 8..index * 8 + 5].copy_from_slice(&[0xc6, 0xc0, 0x81, 0xeb, 0]);
    }
    let mut pure_errors = 0;
    for entries in [false, true] {
        for (bytes, rows, limits, expected) in [
            (
                &ordinary,
                vec![(CODE, 98)],
                CompileLimits {
                    instructions: 32,
                    ..CompileLimits::default()
                },
                CompileError::InstructionLimit,
            ),
            (
                &ordinary,
                vec![(CODE, 98)],
                CompileLimits {
                    wasm_bytes: 1,
                    ..CompileLimits::default()
                },
                CompileError::WasmLimit,
            ),
            (
                &ordinary,
                vec![(CODE, 98)],
                CompileLimits {
                    instructions: 65,
                    ..CompileLimits::default()
                },
                CompileError::InvalidLimits,
            ),
            (
                &nine_source,
                nine_rows.clone(),
                CompileLimits::default(),
                CompileError::InvalidBlocks,
            ),
        ] {
            let engine = code(CODE, bytes);
            let before = inputs(&engine, &[CODE]);
            let memory = engine.memory().unwrap();
            let error = if entries {
                compile_entry_region(
                    memory,
                    &rows
                        .iter()
                        .map(|&(pc, _)| GuestAddress(pc))
                        .collect::<Vec<_>>(),
                    limits,
                )
                .err()
            } else {
                compile_region(
                    memory,
                    &rows
                        .iter()
                        .map(|&(pc, length)| BlockSpec {
                            entry: GuestAddress(pc),
                            byte_length: length,
                        })
                        .collect::<Vec<_>>(),
                    limits,
                )
                .err()
            };
            assert_eq!(error, Some(expected));
            assert_eq!(inputs(&engine, &[CODE]), before);
            pure_errors += 1;
        }
    }
    let mut overflow = Vec::new();
    for _ in 0..64 {
        overflow.extend([0xc6, 0xc4, 0x81]);
    }
    overflow.extend([0xeb, 0]);
    assert_eq!(overflow.len(), 194);
    let engine = code(CODE, &overflow);
    let before = inputs(&engine, &[CODE]);
    let memory = engine.memory().unwrap();
    assert_eq!(
        compile_region(
            memory,
            &[BlockSpec {
                entry: GuestAddress(CODE),
                byte_length: 194
            }],
            CompileLimits::default()
        )
        .err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
        Some(CompileError::InstructionLimit)
    );
    assert_eq!(inputs(&engine, &[CODE]), before);
    pure_errors += 2;
    let mut bound_errors = 0;
    let mut prior_compiles = 0;
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for count_overflow in [false, true] {
                let bytes = if count_overflow {
                    &nine_source
                } else {
                    &overflow
                };
                let (mut engine, resident) = prior_owners(code(CODE, bytes));
                prior_compiles += 2;
                let rows = if count_overflow {
                    nine_rows.clone()
                } else {
                    vec![(CODE, 194)]
                };
                describe(&mut engine, &rows, entries);
                let snapshot = engine
                    .memory()
                    .unwrap()
                    .snapshot_code(GuestAddress(CODE), 4096)
                    .unwrap();
                let before = publication(&engine, resident);
                let expected = if count_overflow {
                    HostError::InvalidRequest
                } else {
                    host_error(owner, CompileError::InstructionLimit)
                };
                assert_eq!(
                    compile(&mut engine, owner, entries, rows.len() as u32),
                    Err(expected)
                );
                retained(&engine, resident, &before, &rows);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                bound_errors += 1;
            }
        }
    }
    assert_eq!((pure_errors, bound_errors, prior_compiles), (10, 8, 16));
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
    assert_module(compiled.wasm_bytes(memory).unwrap(), None, 0, false);
    assert_eq!(inputs(engine, bases), before);
    for owner in [Owner::Replacement, Owner::Resident] {
        describe(engine, &[(pc, length)], entries);
        let before = inputs(engine, bases);
        let id = compile(engine, owner, entries, 1).unwrap();
        assert_eq!(inputs(engine, bases), before);
        guard(engine, owner, id).unwrap();
        assert_module(module(engine, owner, id), Some(owner), id, false);
        if matches!(owner, Owner::Resident) {
            assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
            for offset in 1..length {
                assert!(engine.lookup_resident(pc.wrapping_add(offset)).is_err());
            }
            assert!(engine.lookup_resident(pc.wrapping_add(length)).is_err());
        }
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
    3
}

#[test]
fn every_three_byte_fetch_placement_cut_permission_and_top_extent_is_exact() {
    let mut complete = 0;
    let mut explicit_admissions = 0;
    let mut top_entry_admissions = 0;
    let mut cuts = 0;
    let mut permissions = 0;
    let mut compiler_errors = 0;
    for alias in 0_u8..8 {
        let bytes = [0xc6, 0xc0 | alias, 0x81];
        for pc in [0x1ffd, 0x1ffe, 0x1fff, u32::MAX - 2] {
            let mut engine = code(pc, &bytes);
            assert_decode(
                &engine,
                pc,
                3,
                pc.wrapping_add(3),
                move_byte(usize::from(alias), 0x81),
            );
            unmapped_data(&engine);
            let bases = if matches!(pc, 0x1ffe | 0x1fff) {
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
            if pc == 0x1ffd || pc == u32::MAX - 2 {
                let following = if pc == 0x1ffd { 0x2000 } else { 0 };
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(following), Access::Execute)
                        .is_err()
                );
            }
            explicit_admissions += three_admissions(&mut engine, pc, 3, false, &bases);
            if pc == u32::MAX - 2 {
                let mut entry_engine = code(pc, &bytes);
                top_entry_admissions += three_admissions(&mut entry_engine, pc, 3, true, &bases);
            }
            complete += 1;
        }
        for cut in [1_usize, 2] {
            for top in [false, true] {
                let pc = if top {
                    (1_u64 << 32) - cut as u64
                } else {
                    0x2000 - cut as u64
                } as u32;
                let mut engine = code(pc, &bytes[..cut]);
                let error = fetch_error(
                    pc,
                    if top { pc } else { 0x2000 },
                    cut as u32 + 1,
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
                    3,
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
        }
        for (pc, attempted) in [(0x1fff, 2), (0x1ffe, 3)] {
            let mut engine = code(pc, &bytes);
            engine.protect(0x2000, 1, 3).unwrap();
            let error = fetch_error(pc, 0x2000, attempted, FaultReason::Permission);
            assert_eq!(
                decode_one(engine.memory().unwrap(), GuestAddress(pc)).err(),
                Some(error)
            );
            compiler_errors += six_failures(
                &mut engine,
                pc,
                3,
                error,
                instruction_error(pc, error),
                &[CODE, 0x2000],
            );
            permissions += 1;
        }
    }
    let mut opcode_controls = 0;
    for readable in [false, true] {
        let mut engine = code(CODE, &[0xc6, 0xc4, 0x81]);
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
            3,
            error,
            instruction_error(pc, error),
            &[CODE],
        );
        opcode_controls += 1;
    }
    let mut poisoned = code(CODE, &[0xc6, 0xc4, 0x81, 0x0f, 0x0b]);
    assert_decode(&poisoned, CODE, 3, CODE + 3, move_byte(4, 0x81));
    let consumed_span_admissions = three_admissions(&mut poisoned, CODE, 3, false, &[CODE]);
    assert_eq!(
        (complete, explicit_admissions, top_entry_admissions),
        (32, 96, 24)
    );
    assert_eq!(
        (cuts, permissions, opcode_controls, compiler_errors),
        (32, 16, 2, 300)
    );
    assert_eq!(consumed_span_admissions, 3);
}

#[test]
fn every_consumed_byte_write_stales_both_owners_and_fresh_unsigned_ir_admits() {
    let pc = 0x1ffe;
    let mut scenarios = [0; 3];
    let mut compiles = 0;
    for alias in 0_u8..8 {
        for (offset, new, class) in [
            (0, 0xc6, 0),
            (1, 0xc0 | alias, 0),
            (2, 0x81, 0),
            (2, 0, 1),
            (2, 0xff, 1),
            (1, 0xc0 | ((alias + 1) % 8), 2),
        ] {
            let mut engine = writable_code(pc, &[0xc6, 0xc0 | alias, 0x81]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_decode(&engine, pc, 3, pc + 3, move_byte(usize::from(alias), 0x81));
            describe(&mut engine, &[(pc, 3)], false);
            let before = inputs(&engine, &[CODE, 0x2000]);
            let generation = engine.compile(1).unwrap();
            let resident = engine.compile_resident(1).unwrap().get();
            assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
            engine.guard(KEY, generation).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            assert_module(
                engine.artifact_bytes().unwrap(),
                Some(Owner::Replacement),
                u64::from(generation),
                false,
            );
            assert_module(
                engine.resident_bytes(resident).unwrap(),
                Some(Owner::Resident),
                resident,
                false,
            );
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
            let (fresh_alias, fresh_immediate) = match class {
                0 => (alias, 0x81),
                1 => (alias, new),
                2 => ((alias + 1) % 8, 0x81),
                _ => unreachable!(),
            };
            let fresh = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_eq!(
                fresh.operation(),
                &move_byte(usize::from(fresh_alias), fresh_immediate)
            );
            assert_eq!(
                (fresh.pc(), fresh.length(), fresh.next_pc()),
                (GuestAddress(pc), 3, GuestAddress(pc + 3))
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(fresh.code_snapshot())
            );
            let mut expected_pages = before_pages.clone();
            let address = pc + offset;
            expected_pages[usize::from(address >= 0x2000)][(address & 0xfff) as usize] = new;
            assert_eq!([page(&engine, CODE), page(&engine, 0x2000)], expected_pages);
            if class == 0 {
                assert_eq!(fresh.operation(), decoded.operation());
                assert_eq!(expected_pages, before_pages);
            } else {
                assert_ne!(fresh.operation(), decoded.operation());
            }
            describe(&mut engine, &[(pc, 3)], false);
            let before = inputs(&engine, &[CODE, 0x2000]);
            let fresh_generation = engine.compile(1).unwrap();
            let fresh_resident = engine.compile_resident(1).unwrap().get();
            assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
            engine.guard(KEY, fresh_generation).unwrap();
            engine.guard_resident(KEY, fresh_resident).unwrap();
            assert_module(
                engine.artifact_bytes().unwrap(),
                Some(Owner::Replacement),
                u64::from(fresh_generation),
                false,
            );
            assert_module(
                engine.resident_bytes(fresh_resident).unwrap(),
                Some(Owner::Resident),
                fresh_resident,
                false,
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(fresh.code_snapshot())
            );
            assert_eq!(engine.lookup_resident(pc).unwrap().get(), fresh_resident);
            for offset in 1..=3 {
                assert!(engine.lookup_resident(pc + offset).is_err());
            }
            unmapped_data(&engine);
            scenarios[class] += 1;
            compiles += 4;
        }
    }
    assert_eq!(scenarios, [24, 16, 8]);
    assert_eq!(compiles, 192);
}

#[test]
fn unrelated_data_map_upload_protect_unmap_and_remap_preserve_code_and_owners() {
    let pc = 0x1ffe;
    let mut scenarios = 0;
    let mut compiles = 0;
    for alias in 0_u8..8 {
        let mut engine = code(pc, &[0xc6, 0xc0 | alias, 0x81]);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_decode(&engine, pc, 3, pc + 3, move_byte(usize::from(alias), 0x81));
        describe(&mut engine, &[(pc, 3)], false);
        let before = inputs(&engine, &[CODE, 0x2000]);
        let generation = engine.compile(1).unwrap();
        let resident = engine.compile_resident(1).unwrap().get();
        assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
        engine.guard(KEY, generation).unwrap();
        engine.guard_resident(KEY, resident).unwrap();
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
            for offset in 1..=3 {
                assert!(engine.lookup_resident(pc + offset).is_err());
            }
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
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Read)
                        .is_ok()
                );
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Execute)
                        .is_err()
                );
            }
        }
        compiles += 2;
        scenarios += 1;
    }
    assert_eq!((scenarios, compiles), (8, 16));
}

#[test]
fn ninety_six_late_failures_preserve_complete_publication_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut failures = 0;
    let mut prior_compiles = 0;
    for alias in 0_u8..8 {
        let destination = 0xc0 | alias;
        for (pc, bytes, length, expected) in [
            (
                CODE,
                vec![0xc6, destination, 0x81, 0x0f, 0x0b],
                5,
                instruction_error(CODE + 3, opcode),
            ),
            (
                CODE,
                vec![0xc6, destination, 0x81, 0x66, 0xc6, destination, 0x81],
                7,
                instruction_error(CODE + 3, opcode),
            ),
            (
                0x1ffb,
                vec![0xc6, destination, 0x81, 0xc6, destination],
                6,
                instruction_error(
                    0x1ffe,
                    fetch_error(0x1ffe, 0x2000, 3, FaultReason::Unmapped),
                ),
            ),
        ] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let (mut engine, resident) = prior_owners(code(pc, &bytes));
                    prior_compiles += 2;
                    assert_decode(&engine, pc, 3, pc + 3, move_byte(usize::from(alias), 0x81));
                    describe(&mut engine, &[(pc, length)], entries);
                    let snapshot = engine
                        .memory()
                        .unwrap()
                        .snapshot_code(GuestAddress(CODE), 4096)
                        .unwrap();
                    let before = publication(&engine, resident);
                    assert_eq!(
                        compile(&mut engine, owner, entries, 1),
                        Err(host_error(owner, expected))
                    );
                    retained(&engine, resident, &before, &[(pc, length)]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshot));
                    failures += 1;
                }
            }
        }
    }
    assert_eq!((failures, prior_compiles), (96, 192));
}
