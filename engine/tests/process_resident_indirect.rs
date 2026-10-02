use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            RegistryLimits, ResidentRegistry, compile_entry_region, compile_region,
            prepare_entry_region, prepare_region,
        },
        x86::{
            Register32,
            decode::{DecodeError, decode_one},
            ir::{BranchTarget, EffectiveAddress, Location32, Operation},
        },
    },
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x1020_3040_5060_7080;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const DATA: u32 = 0x5000;

#[test]
fn resident_register_jump_admits_without_target_mapping_or_legacy_artifact() {
    let mut engine = EngineInstance::new(2, 0x1020_3040_5060_7080).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[0x90, 0xff, 0xe0]);
    engine.upload(0x1000, 3).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&3_u32.to_le_bytes());
    let arena = engine.arena().to_vec();
    let id = engine
        .compile_resident(1)
        .expect("resident register jump admission");
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(engine.lookup_resident(0x1001), Ok(id));
    assert_eq!(
        engine.guard_resident(0x1020_3040_5060_7080, id.get()),
        Ok(())
    );
}

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
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    keep: Vec<u8>,
    keep_pointer: usize,
}

fn saved(engine: &EngineInstance, keep: u64) -> Saved {
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    let bytes = engine.resident_bytes(keep).unwrap();
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        generation: engine.generation(),
        legacy: engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        keep: bytes.to_vec(),
        keep_pointer: bytes.as_ptr() as usize,
    }
}

fn error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn assert_unit(engine: &EngineInstance, id: u64, bytes: &[u8], pointer: usize) {
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
    let current = engine.resident_bytes(id).unwrap();
    assert_eq!(current, bytes);
    assert_eq!(current.as_ptr() as usize, pointer);
    assert_eq!(engine.lookup_resident(CODE).unwrap().get(), id);
    assert_eq!(engine.lookup_resident(CODE + 1).unwrap().get(), id);
}

fn admitted(instruction: &[u8], target: Location32) {
    let mut code = vec![0x90];
    code.extend_from_slice(instruction);
    let (mut engine, keep) = fixture(&code);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE + 1))
            .unwrap()
            .operation(),
        &Operation::Jump {
            target: BranchTarget::Indirect(target),
        }
    );
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), code.len())
        .unwrap();
    describe(&mut engine, CODE, code.len());
    let before = saved(&engine, keep);
    let id = engine.compile_resident(1).unwrap().get();
    assert_ne!(id, keep);
    assert_eq!(saved(&engine, keep), before);
    let bytes = engine.resident_bytes(id).unwrap().to_vec();
    let pointer = engine.resident_bytes(id).unwrap().as_ptr() as usize;
    assert_unit(&engine, id, &bytes, pointer);
    assert_eq!(
        engine.lookup_resident(CODE + 2),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(CODE + 2),
        }))
    );
    assert_eq!(
        bytes.windows(6).any(|window| window == b"read32"),
        matches!(target, Location32::Memory(_))
    );
    assert!(!bytes.windows(7).any(|window| window == b"store32"));

    let spec = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: code.len() as u32,
    }];
    let memory = engine.memory().unwrap();
    let limits = CompileLimits::default();
    let expected = error(CODE + 1, InstructionError::BackendUnsupported);
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
        registry.compile(memory, &spec, limits),
        Err(RegistryError::Compile(expected))
    );
    assert_eq!(registry.usage(), usage);
    for pc in [0, DATA, 0x8000, u32::MAX] {
        assert!(memory.resolve(GuestAddress(pc), Access::Read).is_err());
        assert!(memory.resolve(GuestAddress(pc), Access::Execute).is_err());
    }
    assert_eq!(saved(&engine, keep), before);

    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile(1), Ok(2));
    assert_eq!(engine.arena(), arena);
    assert_unit(&engine, id, &bytes, pointer);
    engine.arena_mut().unwrap()[140..].fill(0xa5);
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&CODE.to_le_bytes());
    let before = saved(&engine, keep);
    assert_eq!(engine.compile_entries(1, 0), Ok(3));
    let after = saved(&engine, keep);
    assert_eq!(after.arena, before.arena);
    assert_eq!(after.arena_pointer, before.arena_pointer);
    assert_eq!(after.keep, before.keep);
    assert_eq!(after.keep_pointer, before.keep_pointer);
    assert_unit(&engine, id, &bytes, pointer);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    let mut actual = vec![0; code.len()];
    engine
        .memory()
        .unwrap()
        .fetch(GuestAddress(CODE), &mut actual)
        .unwrap();
    assert_eq!(actual, code);
}

#[test]
fn all_registers_and_eight_memory_eas_preserve_profiles_without_target_access() {
    use Register32::*;
    for (index, register) in [Eax, Ecx, Edx, Ebx, Esp, Ebp, Esi, Edi]
        .into_iter()
        .enumerate()
    {
        admitted(&[0xff, 0xe0 + index as u8], Location32::Register(register));
    }
    let memory = [
        (&[0xff, 0x23][..], Some(Ebx), None, 1, 0),
        (&[0xff, 0x24, 0x24], Some(Esp), None, 1, 0),
        (&[0xff, 0x64, 0x24, 0xfc], Some(Esp), None, 1, u32::MAX - 3),
        (&[0xff, 0x65, 0], Some(Ebp), None, 1, 0),
        (&[0xff, 0x64, 0x8b, 0x10], Some(Ebx), Some(Ecx), 4, 16),
        (
            &[0xff, 0x24, 0xcd, 0, 0x80, 0, 0],
            None,
            Some(Ecx),
            8,
            0x8000,
        ),
        (&[0xff, 0x25, 0, 0x80, 0, 0], None, None, 1, 0x8000),
        (
            &[0xff, 0xa4, 0xf4, 0, 0, 0, 0x80],
            Some(Esp),
            Some(Esi),
            8,
            0x8000_0000,
        ),
    ];
    for (instruction, base, index, scale, displacement) in memory {
        admitted(
            instruction,
            Location32::Memory(EffectiveAddress {
                base,
                index,
                scale,
                displacement,
            }),
        );
    }
}

fn rejected(
    engine: &mut EngineInstance,
    keep: u64,
    bytes: &[u8],
    length: usize,
    cause: InstructionError,
) {
    upload(engine, CODE, bytes);
    describe(engine, CODE, length);
    let before = saved(engine, keep);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(error(
            CODE + 1,
            cause
        ))))
    );
    assert_eq!(saved(engine, keep), before);
}

#[test]
fn jump_span_priority_and_adjacent_exclusions_preserve_previous_state() {
    let (mut engine, keep) = fixture(&[0x90, 0xeb, 0]);
    for instruction in [&[0xff, 0xe0][..], &[0xff, 0x24, 0x24]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            InstructionError::InvalidBlockEnd,
        );
        rejected(
            &mut engine,
            keep,
            &bytes,
            instruction.len(),
            InstructionError::InvalidBlockEnd,
        );
        let memory = engine.memory().unwrap();
        let spec = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let expected = error(CODE + 1, InstructionError::InvalidBlockEnd);
        assert_eq!(
            prepare_region(memory, &spec, CompileLimits::default()).err(),
            Some(expected)
        );
        assert_eq!(
            compile_region(memory, &spec, CompileLimits::default()).err(),
            Some(expected)
        );
        let expected = error(CODE + 1, InstructionError::BackendUnsupported);
        assert_eq!(
            prepare_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(expected)
        );
        assert_eq!(
            compile_entry_region(memory, &[GuestAddress(CODE)], CompileLimits::default()).err(),
            Some(expected)
        );
    }
    for instruction in [
        &[0xff, 0x10][..],
        &[0xe8, 0, 0, 0, 0],
        &[0xff, 0xd0],
        &[0xff, 0x13],
        &[0xc3],
        &[0xc2, 8, 0],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            InstructionError::InvalidBlockEnd,
        );
    }
    for (instruction, feature) in [
        (&[0xff, 0x2b][..], UnsupportedFeature::Opcode),
        (&[0x66, 0xff, 0xe0], UnsupportedFeature::Opcode),
        (&[0x67, 0xff, 0x23], UnsupportedFeature::Opcode),
        (&[0x64, 0xff, 0x23], UnsupportedFeature::Segment),
        (&[0x0f, 0x0b], UnsupportedFeature::Opcode),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            InstructionError::Decode(DecodeError::Unsupported(feature)),
        );
    }
    rejected(
        &mut engine,
        keep,
        &[0x90, 0xf0, 0xff, 0x23],
        4,
        InstructionError::Decode(DecodeError::InvalidEncoding),
    );
}

#[test]
fn live_pointer_data_keeps_units_current_and_code_mutations_require_fresh_identity() {
    let code = [0x90, 0xff, 0x25, 0, 0x50, 0, 0];
    for mutation in 0..3 {
        let (mut engine, keep) = fixture(&code);
        describe(&mut engine, CODE, code.len());
        let id = engine.compile_resident(1).unwrap().get();
        let bytes = engine.resident_bytes(id).unwrap().to_vec();
        let pointer = engine.resident_bytes(id).unwrap().as_ptr() as usize;
        engine.map(DATA, 1, 3).unwrap();
        for target in [KEEP, KEEP + 1, KEEP + 2, 0, u32::MAX] {
            engine.write32(DATA, target).unwrap();
            engine.read32(DATA).unwrap();
            assert_eq!(
                u32::from_le_bytes(engine.arena()[120..124].try_into().unwrap()),
                target
            );
            assert_unit(&engine, id, &bytes, pointer);
            assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
        }
        engine.protect(DATA, 1, 1).unwrap();
        assert_unit(&engine, id, &bytes, pointer);
        engine.unmap(DATA, 1).unwrap();
        assert_unit(&engine, id, &bytes, pointer);
        engine.map(DATA, 1, 3).unwrap();
        assert_unit(&engine, id, &bytes, pointer);
        match mutation {
            0 => upload(&mut engine, CODE, &code),
            1 => engine.protect(CODE, 1, 3).unwrap(),
            _ => {
                engine.unmap(CODE, 1).unwrap();
                engine.map(CODE, 1, 7).unwrap();
            }
        }
        let stale = Err(HostError::Resident(RegistryError::CodeInvalidated));
        assert_eq!(engine.guard_resident(KEY, id), stale);
        assert_eq!(engine.resident_bytes(id).map(|_| ()), stale);
        assert_eq!(
            engine.lookup_resident(CODE),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
        engine.protect(CODE, 1, 7).unwrap();
        upload(&mut engine, CODE, &code);
        describe(&mut engine, CODE, code.len());
        let before = saved(&engine, keep);
        let fresh = engine.compile_resident(1).unwrap().get();
        assert_eq!(saved(&engine, keep), before);
        assert!(fresh > id);
        assert_eq!(engine.lookup_resident(CODE).unwrap().get(), fresh);
        assert_eq!(engine.lookup_resident(CODE + 1).unwrap().get(), fresh);
        assert_eq!(engine.guard_resident(KEY, fresh), Ok(()));
        assert_eq!(engine.guard_resident(KEY, id), stale);
    }
}
