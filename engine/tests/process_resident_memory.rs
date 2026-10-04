use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            RegistryLimits, ResidentRegistry, compile_entry_region, compile_region,
            prepare_entry_region, prepare_region,
        },
        x86::decode::DecodeError,
    },
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x1020_3040_5060_7080;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const DATA: u32 = 0x5000;

const READS: [(&str, &[u8]); 17] = [
    ("mov", &[0x8b, 0x03]),
    ("moffs", &[0xa1, 0x00, 0x50, 0, 0]),
    ("movzx8", &[0x0f, 0xb6, 0x03]),
    ("movsx8", &[0x0f, 0xbe, 0x03]),
    ("movzx16", &[0x0f, 0xb7, 0x03]),
    ("movsx16", &[0x0f, 0xbf, 0x03]),
    ("add", &[0x03, 0x03]),
    ("sub", &[0x2b, 0x03]),
    ("cmp_register", &[0x3b, 0x03]),
    ("and", &[0x23, 0x03]),
    ("or", &[0x0b, 0x03]),
    ("xor", &[0x33, 0x03]),
    ("cmp_memory_register", &[0x39, 0x03]),
    ("cmp_memory_imm32", &[0x81, 0x3b, 0x78, 0x56, 0x34, 0x12]),
    ("cmp_memory_imm8", &[0x83, 0x3b, 0x80]),
    ("test_memory_register", &[0x85, 0x03]),
    ("test_memory_imm32", &[0xf7, 0x03, 0x78, 0x56, 0x34, 0x12]),
];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, pc: u32, length: usize) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    transfer[4..8].copy_from_slice(&(length as u32).to_le_bytes());
}

fn fixture(code: &[u8]) -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for pc in [CODE, KEEP] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, CODE, code);
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    describe(&mut engine, KEEP, 3);
    assert_eq!(engine.compile(1), Ok(1));
    let keep = engine.compile_resident(1).unwrap().get();
    (engine, keep)
}

#[derive(Debug, PartialEq, Eq)]
struct SavedUnit {
    id: u64,
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    units: Vec<SavedUnit>,
}

fn saved(engine: &EngineInstance, ids: &[u64]) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        legacy: engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        units: ids
            .iter()
            .map(|&id| SavedUnit {
                id,
                bytes: engine.resident_bytes(id).map(|bytes| bytes.to_vec()),
                pointer: engine
                    .resident_bytes(id)
                    .ok()
                    .map(|bytes| bytes.as_ptr() as usize),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
    }
}

fn compile_error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn rejected(engine: &mut EngineInstance, keep: u64, bytes: &[u8], cause: InstructionError) {
    upload(engine, CODE, bytes);
    describe(engine, CODE, bytes.len());
    let before = saved(engine, &[keep]);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(compile_error(
            CODE + 1,
            cause,
        ))))
    );
    assert_eq!(saved(engine, &[keep]), before);
}

#[test]
fn resident_mov_load_admits_without_a_data_mapping_or_legacy_artifact() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    let code = [0x8b, 0x03, 0xeb, 0];
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&code);
    engine.upload(CODE, 4).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&CODE.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&4_u32.to_le_bytes());
    let arena = engine.arena().to_vec();
    let id = engine
        .compile_resident(1)
        .expect("resident data read admission");
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(engine.lookup_resident(CODE), Ok(id));
    assert_eq!(engine.guard_resident(KEY, id.get()), Ok(()));
    assert!(!engine.resident_bytes(id.get()).unwrap().is_empty());
}

#[test]
fn all_seventeen_reads_admit_without_data_and_preserve_legacy_and_retained_units() {
    for (name, instruction) in READS {
        let mut code = instruction.to_vec();
        code.extend_from_slice(&[0xeb, 0]);
        let (mut engine, keep) = fixture(&code);
        describe(&mut engine, CODE, code.len());
        let before = saved(&engine, &[keep]);
        let id = engine
            .compile_resident(1)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(saved(&engine, &[keep]), before, "{name}");
        assert_ne!(id.get(), keep);
        assert_eq!(engine.lookup_resident(CODE), Ok(id));
        assert_eq!(
            engine.lookup_resident(CODE + instruction.len() as u32),
            Ok(id)
        );
        assert_eq!(engine.guard_resident(KEY, id.get()), Ok(()));
        let bytes = engine.resident_bytes(id.get()).unwrap();
        assert!(!bytes.windows(7).any(|window| window == b"store32"));
        let read = match name {
            "movzx8" | "movsx8" => &b"read8"[..],
            "movzx16" | "movsx16" => &b"read16"[..],
            _ => &b"read32"[..],
        };
        assert!(
            bytes.windows(read.len()).any(|window| window == read),
            "{name}"
        );

        let spec = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: code.len() as u32,
        }];
        let limits = CompileLimits::default();
        let expected = compile_error(CODE, InstructionError::BackendUnsupported);
        let memory = engine.memory().unwrap();
        assert_eq!(prepare_region(memory, &spec, limits).err(), Some(expected));
        assert_eq!(compile_region(memory, &spec, limits).err(), Some(expected));
        assert_eq!(
            prepare_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
            Some(expected)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], limits).err(),
            Some(expected)
        );
        let mut registry = ResidentRegistry::new(memory, RegistryLimits::default()).unwrap();
        let usage = registry.usage();
        assert_eq!(
            registry.compile(memory, &spec, limits).err(),
            Some(RegistryError::Compile(expected))
        );
        assert_eq!(registry.usage(), usage);
        assert_eq!(saved(&engine, &[keep]), before);

        let arena = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(2), "legacy explicit {name}");
        assert_eq!(engine.arena(), arena);
        engine.arena_mut().unwrap()[140..].fill(0xa5);
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&CODE.to_le_bytes());
        let before = saved(&engine, &[keep, id.get()]);
        assert_eq!(engine.compile_entries(1, 0), Ok(3), "legacy entry {name}");
        let after = saved(&engine, &[keep, id.get()]);
        assert_eq!(after.arena, before.arena);
        assert_eq!(after.arena_pointer, before.arena_pointer);
        assert_eq!(after.units, before.units);
        let mut actual = vec![0; code.len()];
        engine
            .memory()
            .unwrap()
            .fetch(GuestAddress(CODE), &mut actual)
            .unwrap();
        assert_eq!(actual, code);
    }
}

#[test]
fn excluded_operations_and_poison_keep_original_pc_and_block_end_priority() {
    let (mut engine, keep) = fixture(&[0x90, 0xeb, 0]);
    let nonterminal: &[&[u8]] = &[
        &[0xe8, 0, 0, 0, 0],
        &[0xff, 0xd0],
        &[0xff, 0x13],
        &[0xc3],
        &[0xc2, 8, 0],
    ];
    for instruction in nonterminal {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.push(0x90);
        rejected(&mut engine, keep, &bytes, InstructionError::InvalidBlockEnd);
    }
    rejected(
        &mut engine,
        keep,
        &[0x90, 0x0f, 0x0b],
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    );
    for instruction in [
        &[0x66, 0x8b, 0x03][..],
        &[0x67, 0x8b, 0x03],
        &[0x8a, 0xc3],
        &[0xf0, 0x01, 0x03],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        rejected(
            &mut engine,
            keep,
            &bytes,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    for instruction in [&[0xff, 0x10][..], &[0xe8, 0, 0, 0, 0], &[0xc3]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        rejected(&mut engine, keep, &bytes, InstructionError::InvalidBlockEnd);
    }
    rejected(
        &mut engine,
        keep,
        &[0x90, 0xff, 0xe0, 0x0f, 0x06],
        InstructionError::InvalidBlockEnd,
    );
    upload(&mut engine, CODE, &[0x90, 0x8b, 0x03, 0xeb, 0]);
    describe(&mut engine, CODE, 2);
    let before = saved(&engine, &[keep]);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(compile_error(
            CODE + 1,
            InstructionError::InvalidBlockEnd
        ))))
    );
    assert_eq!(saved(&engine, &[keep]), before);
    upload(&mut engine, CODE, &[0x90, 0x8b, 0x03, 0x0f, 0x06]);
    describe(&mut engine, CODE, 5);
    let before = saved(&engine, &[keep]);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(compile_error(
            CODE + 3,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged))
        ))))
    );
    assert_eq!(saved(&engine, &[keep]), before);
}

#[test]
fn data_mapping_and_content_changes_keep_identity_while_code_changes_stale_only_the_read_unit() {
    let code = [0xa1, 0x00, 0x50, 0, 0, 0xeb, 0];
    let (mut engine, keep) = fixture(&code);
    describe(&mut engine, CODE, code.len());
    let old = engine.compile_resident(1).unwrap();
    let bytes = engine.resident_bytes(old.get()).unwrap().to_vec();
    let pointer = engine.resident_bytes(old.get()).unwrap().as_ptr();
    engine.map(DATA, 1, 3).unwrap();
    for value in [0x89ab_cdef, 0x0123_4567] {
        engine.write32(DATA, value).unwrap();
        engine.read32(DATA).unwrap();
        assert_eq!(
            u32::from_le_bytes(engine.arena()[120..124].try_into().unwrap()),
            value
        );
        let before = saved(&engine, &[keep, old.get()]);
        assert_eq!(engine.lookup_resident(CODE), Ok(old));
        assert_eq!(saved(&engine, &[keep, old.get()]), before);
        assert_eq!(engine.resident_bytes(old.get()).unwrap(), bytes);
        assert_eq!(engine.resident_bytes(old.get()).unwrap().as_ptr(), pointer);
    }
    engine.protect(DATA, 1, 1).unwrap();
    engine.unmap(DATA, 1).unwrap();
    assert_eq!(engine.guard_resident(KEY, old.get()), Ok(()));
    engine.map(DATA, 1, 3).unwrap();
    engine.write32(DATA, 0xffff_ffff).unwrap();
    assert_eq!(engine.lookup_resident(CODE + 5), Ok(old));
    assert_eq!(engine.resident_bytes(old.get()).unwrap().as_ptr(), pointer);
    upload(&mut engine, CODE, &code);
    let before = saved(&engine, &[keep, old.get()]);
    assert_eq!(
        engine.lookup_resident(CODE),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(
        engine.guard_resident(KEY, old.get()),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.lookup_resident(KEEP).unwrap().get(), keep);
    assert_eq!(saved(&engine, &[keep, old.get()]), before);
    describe(&mut engine, CODE, code.len());
    let before = saved(&engine, &[keep, old.get()]);
    let fresh = engine.compile_resident(1).unwrap();
    assert_eq!(saved(&engine, &[keep, old.get()]), before);
    assert!(fresh.get() > old.get());
    assert_eq!(engine.lookup_resident(CODE), Ok(fresh));
    assert_eq!(engine.lookup_resident(CODE + 5), Ok(fresh));
    assert_ne!(engine.resident_bytes(fresh.get()).unwrap(), bytes);
    assert_eq!(
        engine.guard_resident(KEY, old.get()),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
}
