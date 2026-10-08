use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, CompiledRegion, InstructionError,
            RegistryError, compile_entry_region, compile_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BinaryKind, ByteRegister, Condition, Location32, Operation, UnaryKind, Value32},
        },
    },
    memory::{Access, CodeSnapshot, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError, ResidentInstallation},
};

#[test]
fn xadd_eax_ecx_admits_before_jump_with_unmapped_data() {
    let mut engine = EngineInstance::new(1, 0xcadd_0000_f123_4567).unwrap();
    let bytes = [0x0f, 0xc1, 0xc8, 0xeb, 0];
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
        .expect("register XADD must admit before a jump without reading data");
}

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const DATA: u32 = 0x5000;
const KEY: u64 = 0xcadd_0000_f123_4567;
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
const WITNESSES: [u8; 3] = [0xc0, 0xc8, 0xc1];

#[derive(Debug, Default, PartialEq, Eq)]
struct Census {
    pure_success: usize,
    bound_success: usize,
    pure_error: usize,
    bound_error: usize,
}

fn assert_census(c: &Census, expected: (usize, usize, usize, usize)) {
    assert_eq!(
        (c.pure_success, c.bound_success, c.pure_error, c.bound_error),
        expected
    );
}

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

fn snapshots(engine: &EngineInstance, rows: &[(u32, u32)]) -> Vec<CodeSnapshot> {
    let mut bases = std::collections::BTreeSet::from([CODE, KEEP, DATA]);
    for &(pc, _) in rows {
        let base = pc & !0xfff;
        bases.insert(base);
        if let Some(next) = base.checked_add(4096) {
            bases.insert(next);
        }
    }
    bases
        .into_iter()
        .filter_map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .ok()
        })
        .collect()
}

fn assert_snapshots(engine: &EngineInstance, snapshots: &[CodeSnapshot]) {
    for snapshot in snapshots {
        assert!(engine.memory().unwrap().is_code_current(snapshot));
    }
}

fn compile(
    engine: &mut EngineInstance,
    owner: Owner,
    entries: bool,
    count: u32,
    census: &mut Census,
) -> Result<u64, HostError> {
    let transfer = &engine.arena()[TRANSFER_OFFSET..];
    let stride = if entries { 4 } else { 8 };
    let rows: Vec<_> = (0..count as usize)
        .map(|i| {
            let at = i * stride;
            (
                u32::from_le_bytes(transfer[at..at + 4].try_into().unwrap()),
                0,
            )
        })
        .collect();
    let snapshots = snapshots(engine, &rows);
    let result = match (owner, entries) {
        (Owner::Replacement, false) => engine.compile(count).map(u64::from),
        (Owner::Replacement, true) => engine.compile_entries(count, 0).map(u64::from),
        (Owner::Resident, false) => engine.compile_resident(count).map(|id| id.get()),
        (Owner::Resident, true) => engine.compile_resident_entries(count, 0).map(|id| id.get()),
    };
    assert_snapshots(engine, &snapshots);
    if result.is_ok() {
        census.bound_success += 1;
    } else {
        census.bound_error += 1;
    }
    result
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

fn assert_module(wasm: &[u8], owner: Option<Owner>, id: u64) {
    assert!(wasm.len() <= CompileLimits::default().wasm_bytes);
    let resident = matches!(owner, Some(Owner::Resident));
    let arities = if owner.is_some() {
        vec![4, if resident { 7 } else { 6 }]
    } else {
        vec![4]
    };
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
    assert_eq!(unsigned(&mut imports), if owner.is_some() { 2 } else { 1 });
    assert_eq!((name(&mut imports), name(&mut imports)), ("env", "memory"));
    assert_eq!(byte(&mut imports), 2);
    assert_eq!((unsigned(&mut imports), unsigned(&mut imports)), (0, 1));
    if owner.is_some() {
        assert_eq!(
            (name(&mut imports), name(&mut imports)),
            ("ring3", if resident { "guard_resident" } else { "guard" })
        );
        assert_eq!((byte(&mut imports), unsigned(&mut imports)), (0, 1));
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
        (0, u32::from(owner.is_some()))
    );
    assert!(exports.is_empty());
    let mut bodies = section(wasm, 10);
    assert_eq!(unsigned(&mut bodies), 1);
    let length = unsigned(&mut bodies) as usize;
    let mut body = take(&mut bodies, length);
    assert!(bodies.is_empty());
    assert_eq!(unsigned(&mut body), 2);
    for expected in [(16, 0x7f), (1, 0x7e)] {
        assert_eq!((unsigned(&mut body), byte(&mut body)), expected);
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

fn pure(
    engine: &EngineInstance,
    rows: &[(u32, u32)],
    entries: bool,
    limits: CompileLimits,
    census: &mut Census,
) -> Result<CompiledRegion, CompileError> {
    let snapshots = snapshots(engine, rows);
    let memory = engine.memory().unwrap();
    let result = if entries {
        compile_entry_region(
            memory,
            &rows
                .iter()
                .map(|&(pc, _)| GuestAddress(pc))
                .collect::<Vec<_>>(),
            limits,
        )
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
    };
    assert_snapshots(engine, &snapshots);
    if result.is_ok() {
        census.pure_success += 1;
    } else {
        census.pure_error += 1;
    }
    result
}

fn xadd(modrm: u8) -> Operation {
    assert_eq!(modrm >> 6, 3);
    Operation::ExchangeAdd {
        destination: REGISTERS[usize::from(modrm & 7)],
        source: REGISTERS[usize::from((modrm >> 3) & 7)],
    }
}

fn assert_lookup(engine: &EngineInstance, pc: u32, length: u32, starts: &[u32], id: u64) {
    for offset in 0..=length {
        let found = engine.lookup_resident(pc.wrapping_add(offset));
        if starts.contains(&offset) {
            assert_eq!(found.unwrap().get(), id);
        } else {
            assert!(found.is_err());
        }
    }
}

fn six_failures(
    engine: &mut EngineInstance,
    request: (u32, u32),
    error: DecodeError,
    explicit: CompileError,
    bases: &[u32],
    census: &mut Census,
) {
    let (pc, length) = request;
    let entry = instruction_error(pc, error);
    for entries in [false, true] {
        let before = inputs(engine, bases);
        assert_eq!(
            pure(
                engine,
                &[(pc, length)],
                entries,
                CompileLimits::default(),
                census
            )
            .err(),
            Some(if entries { entry } else { explicit })
        );
        assert_eq!(inputs(engine, bases), before);
    }
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            describe(engine, &[(pc, length)], entries);
            let before = inputs(engine, bases);
            let generation = engine.generation();
            assert_eq!(
                compile(engine, owner, entries, 1, census),
                Err(host_error(owner, if entries { entry } else { explicit }))
            );
            assert_eq!(inputs(engine, bases), before);
            assert_eq!(engine.generation(), generation);
            assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
            for offset in 0..=length {
                assert!(engine.lookup_resident(pc.wrapping_add(offset)).is_err());
            }
        }
    }
}

fn rejected(bytes: &[u8], error: DecodeError, census: &mut Census) {
    let mut engine = code(CODE, bytes);
    unmapped_data(&engine);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE)).err(),
        Some(error),
        "{bytes:02x?}"
    );
    six_failures(
        &mut engine,
        (CODE, bytes.len() as u32),
        error,
        instruction_error(CODE, error),
        &[CODE],
        census,
    );
}

fn bank(first: u8, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for ordinal in 0..count {
        bytes.extend([0x0f, 0xc1, first + ordinal as u8]);
    }
    bytes.extend([0xeb, 0]);
    bytes
}

#[test]
fn all_sixty_four_ordered_register_pairs_admit_two_banks_in_six_profiles() {
    let limits = CompileLimits::default();
    assert_eq!(
        (limits.instructions, limits.blocks, limits.wasm_bytes),
        (64, 8, 65536)
    );
    let mut census = Census::default();
    let mut identities = std::collections::BTreeSet::new();
    let mut classes = [0; 2];
    for first in [0xc0, 0xe0] {
        let bytes = bank(first, 32);
        assert_eq!(bytes.len(), 98);
        let engine = code(CODE, &bytes);
        let before = inputs(&engine, &[CODE]);
        unmapped_data(&engine);
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(CODE), Access::Read)
                .is_err()
        );
        for index in 0..32 {
            let raw = first + index as u8;
            assert!(identities.insert(raw));
            let pc = CODE + 3 * index as u32;
            assert_eq!(&bytes[3 * index..3 * index + 3], &[0x0f, 0xc1, raw]);
            assert_decode(&engine, pc, 3, pc + 3, xadd(raw));
            classes[usize::from(raw & 7 != (raw >> 3) & 7)] += 1;
        }
        let extent = pure(&engine, &[(CODE, 98)], false, limits, &mut census).unwrap();
        let entry = pure(&engine, &[(CODE, 98)], true, limits, &mut census).unwrap();
        for compiled in [&extent, &entry] {
            assert_eq!(
                (compiled.metadata().blocks, compiled.metadata().instructions),
                (1, 33)
            );
            assert_module(
                compiled.wasm_bytes(engine.memory().unwrap()).unwrap(),
                None,
                0,
            );
        }
        assert_eq!(extent.metadata(), entry.metadata());
        assert_eq!(
            extent.wasm_bytes(engine.memory().unwrap()).unwrap(),
            entry.wasm_bytes(engine.memory().unwrap()).unwrap()
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
                let id = compile(&mut engine, owner, entries, 1, &mut census).unwrap();
                assert_eq!(inputs(&engine, &[CODE]), before);
                assert!(engine.memory().unwrap().is_code_current(&snapshot));
                guard(&engine, owner, id).unwrap();
                assert_module(module(&engine, owner, id), Some(owner), id);
                unmapped_data(&engine);
                if matches!(owner, Owner::Resident) {
                    assert_lookup(
                        &engine,
                        CODE,
                        98,
                        &(0..=32).map(|i| 3 * i).collect::<Vec<_>>(),
                        id,
                    );
                    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
                    assert_eq!(engine.generation(), 0);
                }
            }
        }
    }
    assert_eq!((identities.len(), classes), (64, [8, 56]));
    assert_census(&census, (4, 8, 0, 0));
}

fn canonical_memory(modrm: u8) -> Vec<u8> {
    let mode = modrm >> 6;
    assert!(mode < 3);
    let rm = modrm & 7;
    let mut bytes = vec![0x66, 0x0f, 0xc1, modrm];
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
    let mut census = Census::default();
    let mut domain = [0; 4];
    let mut negatives = std::collections::BTreeSet::new();
    let mut categories = [0; 3];
    for raw in 0_u16..=255 {
        let modrm = raw as u8;
        domain[usize::from(modrm >> 6)] += 1;
        if modrm >> 6 == 3 {
            continue;
        }
        let bytes = canonical_memory(modrm);
        assert!(negatives.insert(bytes.clone()));
        rejected(&bytes, opcode, &mut census);
        categories[2] += 1;
    }
    assert_eq!(domain, [64; 4]);
    let registers: Vec<u8> = (0..8)
        .map(|i| 0xc0 | i << 3 | i)
        .chain([0xc8, 0xc1])
        .collect();
    let prefixes = [
        0x66, 0x67, 0xf2, 0xf3, 0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0xf0,
    ];
    let mut strict = Vec::new();
    for prefix in prefixes {
        let category = if matches!(prefix, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65) {
            segment
        } else {
            opcode
        };
        for &raw in &registers {
            strict.push((
                vec![prefix, 0x0f, 0xc1, raw],
                if prefix == 0xf0 {
                    DecodeError::InvalidEncoding
                } else {
                    category
                },
            ));
        }
        for memory in [&[0x0f, 0xc1, 0x03][..], &[0x0f, 0xc1, 0x04, 0x24][..]] {
            let mut bytes = vec![prefix];
            if prefix == 0x66 {
                bytes.push(0x66);
            }
            bytes.extend(memory);
            strict.push((bytes, category));
        }
    }
    for &raw in &registers {
        strict.push((vec![0x66, 0x0f, 0xc0, raw], opcode));
    }
    strict.extend([
        (vec![0x66, 0x0f, 0xc0, 0x03], opcode),
        (vec![0x66, 0x0f, 0xc0, 0x04, 0x24], opcode),
        (vec![0x66, 0x0f, 0xb1, 0xc8], opcode),
        (vec![0x66, 0x0f, 0xb1, 0x03], opcode),
        (vec![0x66, 0x87, 0xc8], opcode),
        (vec![0x0f, 0x0b], opcode),
    ]);
    assert_eq!(strict.len(), 148);
    for (bytes, error) in strict {
        assert!(negatives.insert(bytes.clone()));
        categories[match error {
            DecodeError::InvalidEncoding => 0,
            DecodeError::Unsupported(UnsupportedFeature::Segment) => 1,
            DecodeError::Unsupported(UnsupportedFeature::Opcode) => 2,
            _ => panic!("unexpected category"),
        }] += 1;
        rejected(&bytes, error, &mut census);
    }
    assert_eq!((negatives.len(), categories), (340, [10, 72, 258]));
    assert_eq!(domain[3] + negatives.len(), 404);
    assert_census(&census, (0, 0, 680, 1360));
    let mut observations = 0;
    let mut old_neighbors = std::collections::BTreeSet::new();
    assert_eq!(registers.len(), 10);
    for &raw in &registers {
        for (bytes, operation) in [
            (
                vec![0x01, raw],
                Operation::Binary {
                    kind: BinaryKind::Add,
                    destination: Location32::Register(REGISTERS[usize::from(raw & 7)]),
                    source: Value32::Register(REGISTERS[usize::from((raw >> 3) & 7)]),
                },
            ),
            (
                vec![0x87, raw],
                Operation::Exchange {
                    left: REGISTERS[usize::from(raw & 7)],
                    right: REGISTERS[usize::from((raw >> 3) & 7)],
                },
            ),
        ] {
            assert!(old_neighbors.insert(bytes.clone()));
            let engine = code(CODE, &bytes);
            assert_decode(&engine, CODE, 2, CODE + 2, operation);
            observations += 1;
        }
    }
    for (bytes, operation) in [
        (
            vec![0x0f, 0x92, 0xc0],
            Operation::SetByte {
                condition: Condition::Below,
                destination: ByteRegister::Al,
            },
        ),
        (
            vec![0x0f, 0x90, 0xc4],
            Operation::SetByte {
                condition: Condition::Overflow,
                destination: ByteRegister::Ah,
            },
        ),
        (
            vec![0x11, 0xc8],
            Operation::Binary {
                kind: BinaryKind::Adc,
                destination: Location32::Register(Register32::Eax),
                source: Value32::Register(Register32::Ecx),
            },
        ),
        (
            vec![0x19, 0xc8],
            Operation::Binary {
                kind: BinaryKind::Sbb,
                destination: Location32::Register(Register32::Eax),
                source: Value32::Register(Register32::Ecx),
            },
        ),
    ] {
        assert!(old_neighbors.insert(bytes.clone()));
        let engine = code(CODE, &bytes);
        let length = bytes.len() as u8;
        assert_decode(&engine, CODE, length, CODE + u32::from(length), operation);
        observations += 1;
    }
    for (opcode, raw, kind) in [(0x40, 0xc8, UnaryKind::Inc), (0x48, 0xc1, UnaryKind::Dec)] {
        assert!(old_neighbors.insert(vec![opcode]));
        let engine = code(CODE, &[opcode, 0x0f, 0xc1, raw]);
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
        assert_decode(&engine, CODE + 1, 3, CODE + 4, xadd(raw));
        observations += 2;
    }
    assert_eq!((observations, old_neighbors.len()), (28, 26));
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

fn prior_owners(engine: &mut EngineInstance, census: &mut Census) -> u64 {
    engine.map(KEEP, 1, 7).unwrap();
    upload(engine, KEEP, &[0x90, 0xeb, 0]);
    engine.map(DATA, 1, 3).unwrap();
    let data: Vec<u8> = (0..4096).map(|i| (i % 251 + 1) as u8).collect();
    upload(engine, DATA, &data);
    describe(engine, &[(KEEP, 3)], false);
    let before = inputs(engine, &[CODE, KEEP, DATA]);
    let generation = compile(engine, Owner::Replacement, false, 1, census).unwrap();
    assert_eq!(inputs(engine, &[CODE, KEEP, DATA]), before);
    let resident = compile(engine, Owner::Resident, false, 1, census).unwrap();
    assert_eq!(inputs(engine, &[CODE, KEEP, DATA]), before);
    engine.guard(KEY, generation as u32).unwrap();
    engine.guard_resident(KEY, resident).unwrap();
    assert_module(
        engine.artifact_bytes().unwrap(),
        Some(Owner::Replacement),
        generation,
    );
    assert_module(
        engine.resident_bytes(resident).unwrap(),
        Some(Owner::Resident),
        resident,
    );
    engine
        .acknowledge_resident_installation(KEY, resident, 3)
        .unwrap();
    resident
}

fn failed(
    engine: &mut EngineInstance,
    resident: u64,
    owner: Owner,
    entries: bool,
    rows: &[(u32, u32)],
    expected: HostError,
    census: &mut Census,
) {
    describe(engine, rows, entries);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), 4096)
        .unwrap();
    let before = publication(engine, resident);
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
    assert_eq!(
        compile(engine, owner, entries, rows.len() as u32, census),
        Err(expected)
    );
    assert_eq!(publication(engine, resident), before);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    engine.guard(KEY, before.generation).unwrap();
    engine.guard_resident(KEY, resident).unwrap();
    assert_lookup(engine, KEEP, 3, &[0, 1], resident);
    for &(pc, length) in rows {
        for offset in 0..=length {
            assert!(engine.lookup_resident(pc.wrapping_add(offset)).is_err());
        }
    }
}

#[test]
fn exact_caps_and_eight_failures_preserve_complete_installed_publication() {
    let mut census = Census::default();
    let bytes = bank(0xc0, 32);
    let engine = code(CODE, &bytes);
    for entries in [false, true] {
        for (limits, expected) in [
            (
                CompileLimits {
                    instructions: 32,
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
            (
                CompileLimits {
                    instructions: 65,
                    ..CompileLimits::default()
                },
                CompileError::InvalidLimits,
            ),
        ] {
            let before = inputs(&engine, &[CODE]);
            assert_eq!(
                pure(&engine, &[(CODE, 98)], entries, limits, &mut census).err(),
                Some(expected)
            );
            assert_eq!(inputs(&engine, &[CODE]), before);
        }
    }
    let rows: Vec<_> = (0..9).map(|i| (CODE + i * 8, 5)).collect();
    let mut nine = vec![0; 69];
    for i in 0..9 {
        nine[i * 8..i * 8 + 5].copy_from_slice(&[0x0f, 0xc1, 0xc8, 0xeb, 0]);
    }
    let engine = code(CODE, &nine);
    for entries in [false, true] {
        let before = inputs(&engine, &[CODE]);
        assert_eq!(
            pure(
                &engine,
                &rows,
                entries,
                CompileLimits::default(),
                &mut census
            )
            .err(),
            Some(CompileError::InvalidBlocks)
        );
        assert_eq!(inputs(&engine, &[CODE]), before);
    }
    let mut overflow = [0x0f, 0xc1, 0xc8].repeat(64);
    overflow.extend([0xeb, 0]);
    assert_eq!(overflow.len(), 194);
    let engine = code(CODE, &overflow);
    for entries in [false, true] {
        let before = inputs(&engine, &[CODE]);
        assert_eq!(
            pure(
                &engine,
                &[(CODE, 194)],
                entries,
                CompileLimits::default(),
                &mut census
            )
            .err(),
            Some(CompileError::InstructionLimit)
        );
        assert_eq!(inputs(&engine, &[CODE]), before);
    }
    let mut rollbacks = 0;
    for owner in [Owner::Replacement, Owner::Resident] {
        for entries in [false, true] {
            for count_failure in [false, true] {
                let mut engine = code(CODE, if count_failure { &nine } else { &overflow });
                let resident = prior_owners(&mut engine, &mut census);
                let request = if count_failure {
                    rows.clone()
                } else {
                    vec![(CODE, 194)]
                };
                let expected = if count_failure {
                    HostError::InvalidRequest
                } else {
                    host_error(owner, CompileError::InstructionLimit)
                };
                failed(
                    &mut engine,
                    resident,
                    owner,
                    entries,
                    &request,
                    expected,
                    &mut census,
                );
                rollbacks += 1;
            }
        }
    }
    assert_eq!(rollbacks, 8);
    assert_census(&census, (0, 16, 10, 8));
}

fn three_admissions(
    engine: &mut EngineInstance,
    pc: u32,
    entries: bool,
    bases: &[u32],
    census: &mut Census,
) {
    let before = inputs(engine, bases);
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(pc), 3)
        .unwrap();
    let compiled = pure(
        engine,
        &[(pc, 3)],
        entries,
        CompileLimits::default(),
        census,
    )
    .unwrap();
    assert_eq!(
        (compiled.metadata().blocks, compiled.metadata().instructions),
        (1, 1)
    );
    assert_module(
        compiled.wasm_bytes(engine.memory().unwrap()).unwrap(),
        None,
        0,
    );
    assert_eq!(inputs(engine, bases), before);
    for owner in [Owner::Replacement, Owner::Resident] {
        describe(engine, &[(pc, 3)], entries);
        let before = inputs(engine, bases);
        let id = compile(engine, owner, entries, 1, census).unwrap();
        assert_eq!(inputs(engine, bases), before);
        guard(engine, owner, id).unwrap();
        assert_module(module(engine, owner, id), Some(owner), id);
        if matches!(owner, Owner::Resident) {
            assert_lookup(engine, pc, 3, &[0], id);
        }
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
}

#[test]
fn exact_three_byte_fetch_cuts_permissions_top_and_consumed_span_are_preserved() {
    let mut census = Census::default();
    let mut complete = 0;
    let mut cuts = 0;
    let mut permissions = 0;
    for raw in WITNESSES {
        let bytes = [0x0f, 0xc1, raw];
        for pc in [0x1ffd, 0x1ffe, 0x1fff, u32::MAX - 2] {
            let mut engine = code(pc, &bytes);
            assert_decode(&engine, pc, 3, pc.wrapping_add(3), xadd(raw));
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
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(pc.wrapping_add(3)), Access::Execute)
                        .is_err()
                );
            }
            three_admissions(&mut engine, pc, false, &bases, &mut census);
            if pc == u32::MAX - 2 {
                let mut fresh = code(pc, &bytes);
                three_admissions(&mut fresh, pc, true, &bases, &mut census);
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
                six_failures(
                    &mut engine,
                    (pc, 3),
                    error,
                    if top {
                        CompileError::InvalidBlocks
                    } else {
                        instruction_error(pc, error)
                    },
                    &[pc & !0xfff],
                    &mut census,
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
            six_failures(
                &mut engine,
                (pc, 3),
                error,
                instruction_error(pc, error),
                &[CODE, 0x2000],
                &mut census,
            );
            permissions += 1;
        }
    }
    let mut opcode_controls = 0;
    for readable in [false, true] {
        let mut engine = code(CODE, &[0x0f, 0xc1, 0xc8]);
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
        six_failures(
            &mut engine,
            (pc, 3),
            error,
            instruction_error(pc, error),
            &[CODE],
            &mut census,
        );
        opcode_controls += 1;
    }
    let mut poisoned = code(CODE, &[0x0f, 0xc1, 0xc8, 0x0f, 0x0b]);
    assert_decode(&poisoned, CODE, 3, CODE + 3, xadd(0xc8));
    three_admissions(&mut poisoned, CODE, false, &[CODE], &mut census);
    assert_eq!(
        (complete, cuts, permissions, opcode_controls),
        (12, 12, 6, 2)
    );
    assert_census(&census, (16, 32, 40, 80));
}

#[test]
fn every_consumed_byte_and_both_operand_fields_stale_owners_then_fresh_ir_admits() {
    let pc = 0x1ffe;
    let mut census = Census::default();
    let mut kinds = [0; 3];
    for raw in WITNESSES {
        for (offset, new, kind) in [
            (0, 0x0f, 0),
            (1, 0xc1, 0),
            (2, raw, 0),
            (2, raw ^ 1, 1),
            (2, raw ^ 8, 2),
        ] {
            let mut engine = writable_code(pc, &[0x0f, 0xc1, raw]);
            let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            assert_decode(&engine, pc, 3, pc + 3, xadd(raw));
            describe(&mut engine, &[(pc, 3)], false);
            let before = inputs(&engine, &[CODE, 0x2000]);
            let generation =
                compile(&mut engine, Owner::Replacement, false, 1, &mut census).unwrap();
            let resident = compile(&mut engine, Owner::Resident, false, 1, &mut census).unwrap();
            assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
            engine.guard(KEY, generation as u32).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            assert_module(
                engine.artifact_bytes().unwrap(),
                Some(Owner::Replacement),
                generation,
            );
            assert_module(
                engine.resident_bytes(resident).unwrap(),
                Some(Owner::Resident),
                resident,
            );
            let mut expected_pages = [page(&engine, CODE), page(&engine, 0x2000)];
            let mut expected_arena = engine.arena().to_vec();
            expected_arena[TRANSFER_OFFSET] = new;
            upload(&mut engine, pc + offset, &[new]);
            assert_eq!(engine.arena(), expected_arena);
            assert_eq!(engine.arena_address(), before.pointer);
            assert_eq!((engine.is_open(), engine.key()), before.authority);
            assert_eq!(
                engine.guard(KEY, generation as u32),
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
            let address = pc + offset;
            expected_pages[usize::from(address >= 0x2000)][(address & 0xfff) as usize] = new;
            assert_eq!([page(&engine, CODE), page(&engine, 0x2000)], expected_pages);
            let fresh_raw = if offset == 2 { new } else { raw };
            assert_decode(&engine, pc, 3, pc + 3, xadd(fresh_raw));
            let fresh = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
            if kind == 0 {
                assert_eq!(fresh.operation(), decoded.operation());
            } else {
                assert_ne!(fresh.operation(), decoded.operation());
            }
            let Operation::ExchangeAdd {
                destination: old_destination,
                source: old_source,
            } = *decoded.operation()
            else {
                panic!("old target")
            };
            let Operation::ExchangeAdd {
                destination,
                source,
            } = *fresh.operation()
            else {
                panic!("fresh target")
            };
            match kind {
                0 => assert_eq!((destination, source), (old_destination, old_source)),
                1 => {
                    assert_ne!(destination, old_destination);
                    assert_eq!(source, old_source);
                }
                2 => {
                    assert_eq!(destination, old_destination);
                    assert_ne!(source, old_source);
                }
                _ => unreachable!(),
            }
            describe(&mut engine, &[(pc, 3)], false);
            let before = inputs(&engine, &[CODE, 0x2000]);
            let fresh_generation =
                compile(&mut engine, Owner::Replacement, false, 1, &mut census).unwrap();
            let fresh_resident =
                compile(&mut engine, Owner::Resident, false, 1, &mut census).unwrap();
            assert_ne!(fresh_generation, generation);
            assert_ne!(fresh_resident, resident);
            assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
            engine.guard(KEY, fresh_generation as u32).unwrap();
            engine.guard_resident(KEY, fresh_resident).unwrap();
            assert_module(
                engine.artifact_bytes().unwrap(),
                Some(Owner::Replacement),
                fresh_generation,
            );
            assert_module(
                engine.resident_bytes(fresh_resident).unwrap(),
                Some(Owner::Resident),
                fresh_resident,
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .is_code_current(fresh.code_snapshot())
            );
            assert_lookup(&engine, pc, 3, &[0], fresh_resident);
            assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
            unmapped_data(&engine);
            kinds[kind] += 1;
        }
    }
    assert_eq!(kinds, [9, 3, 3]);
    assert_census(&census, (0, 60, 0, 0));
}

#[test]
fn unrelated_data_map_upload_protect_unmap_and_remap_preserve_code_and_owners() {
    let pc = 0x1ffe;
    let mut census = Census::default();
    let mut scenarios = 0;
    for raw in WITNESSES {
        let mut engine = code(pc, &[0x0f, 0xc1, raw]);
        let decoded = decode_one(engine.memory().unwrap(), GuestAddress(pc)).unwrap();
        assert_decode(&engine, pc, 3, pc + 3, xadd(raw));
        describe(&mut engine, &[(pc, 3)], false);
        let before = inputs(&engine, &[CODE, 0x2000]);
        let generation = compile(&mut engine, Owner::Replacement, false, 1, &mut census).unwrap();
        let resident = compile(&mut engine, Owner::Resident, false, 1, &mut census).unwrap();
        assert_eq!(inputs(&engine, &[CODE, 0x2000]), before);
        assert_module(
            engine.artifact_bytes().unwrap(),
            Some(Owner::Replacement),
            generation,
        );
        assert_module(
            engine.resident_bytes(resident).unwrap(),
            Some(Owner::Resident),
            resident,
        );
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
            engine.guard(KEY, generation as u32).unwrap();
            engine.guard_resident(KEY, resident).unwrap();
            assert_eq!(u64::from(engine.generation()), generation);
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
            assert_lookup(&engine, pc, 3, &[0], resident);
            if stage == 3 {
                assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
                for access in [Access::Read, Access::Write, Access::Execute] {
                    assert!(
                        engine
                            .memory()
                            .unwrap()
                            .resolve(GuestAddress(DATA), access)
                            .is_err()
                    );
                }
            } else {
                assert_eq!(engine.memory().unwrap().mapped_pages(), 3);
                let mut expected = vec![0; 4096];
                if matches!(stage, 1 | 2 | 5 | 6) {
                    expected[0x10] = if stage < 4 { 0x80 } else { 0x81 };
                }
                assert_eq!(page(&engine, DATA), expected);
                assert!(
                    engine
                        .memory()
                        .unwrap()
                        .resolve(GuestAddress(DATA), Access::Read)
                        .is_ok()
                );
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
                        .resolve(GuestAddress(DATA), Access::Execute)
                        .is_err()
                );
            }
        }
        scenarios += 1;
    }
    assert_eq!(scenarios, 3);
    assert_census(&census, (0, 6, 0, 0));
}

#[test]
fn thirty_six_late_failures_preserve_complete_two_owner_publication_and_installation() {
    let opcode = DecodeError::Unsupported(UnsupportedFeature::Opcode);
    let mut census = Census::default();
    let mut failures = 0;
    for raw in WITNESSES {
        for (pc, bytes, length, expected) in [
            (
                CODE,
                vec![0x0f, 0xc1, raw, 0x0f, 0x0b],
                5,
                instruction_error(CODE + 3, opcode),
            ),
            (
                CODE,
                vec![0x0f, 0xc1, raw, 0x66, 0x0f, 0xc1, raw],
                7,
                instruction_error(CODE + 3, opcode),
            ),
            (
                0x1ffb,
                vec![0x0f, 0xc1, raw, 0x0f, 0xc1],
                6,
                instruction_error(
                    0x1ffe,
                    fetch_error(0x1ffe, 0x2000, 3, FaultReason::Unmapped),
                ),
            ),
        ] {
            for owner in [Owner::Replacement, Owner::Resident] {
                for entries in [false, true] {
                    let mut engine = code(pc, &bytes);
                    assert_decode(&engine, pc, 3, pc + 3, xadd(raw));
                    let resident = prior_owners(&mut engine, &mut census);
                    failed(
                        &mut engine,
                        resident,
                        owner,
                        entries,
                        &[(pc, length)],
                        host_error(owner, expected),
                        &mut census,
                    );
                    failures += 1;
                }
            }
        }
    }
    assert_eq!(failures, 36);
    assert_census(&census, (0, 72, 0, 36));
}
