use ring3_engine::process::{EngineInstance, HostError};

#[test]
fn resident_mov_store_admits_without_data_mapping_or_legacy_artifact() {
    let mut engine = EngineInstance::new(2, 0x1020_3040_5060_7080).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&[0x89, 0x03, 0xeb, 0]);
    engine.upload(0x1000, 4).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&4_u32.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = engine
        .compile_resident(1)
        .expect("resident MOV store admission");
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(engine.lookup_resident(0x1000), Ok(id));
    assert_eq!(
        engine.guard_resident(0x1020_3040_5060_7080, id.get()),
        Ok(())
    );
}

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
            ir::{EffectiveAddress, Location32, Operation, Value32},
        },
    },
    memory::{Access, GuestAddress},
    process::StoreCompletion,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const LEGACY: u32 = 0x3000;
const OTHER: u32 = 0x4000;
const DATA: u32 = 0x5000;
const MOV: [u8; 4] = [0x89, 0x03, 0xeb, 0];

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, specs: &[(u32, usize)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    for (index, &(pc, length)) in specs.iter().enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&(length as u32).to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, specs: &[(u32, usize)]) -> u64 {
    describe(engine, specs);
    let arena = engine.arena().to_vec();
    let id = engine.compile_resident(specs.len() as u32).unwrap().get();
    assert_eq!(engine.arena(), arena);
    id
}

fn fixture(code: &[u8]) -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for pc in [CODE, KEEP, LEGACY] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, CODE, code);
    upload(&mut engine, KEEP, &[0x90, 0xeb, 0]);
    upload(&mut engine, LEGACY, &[0x90, 0xeb, 0]);
    describe(&mut engine, &[(LEGACY, 3)]);
    assert_eq!(engine.compile(1), Ok(1));
    let keep = compile(&mut engine, &[(KEEP, 3)]);
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

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

fn helper(fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn store(
    engine: &mut EngineInstance,
    id: u64,
    address: u32,
    value: u32,
    completion: StoreCompletion,
    fields: [u32; 6],
) {
    let mut arena = engine.arena().to_vec();
    arena[100..140].copy_from_slice(&helper(fields));
    let pointer = engine.arena_address();
    let generation = engine.generation();
    assert_eq!(
        engine.store_resident32(KEY, id, address, value),
        Ok(completion)
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.arena_address(), pointer);
    assert_eq!(engine.generation(), generation);
}

fn error(pc: u32, cause: InstructionError) -> HostError {
    HostError::Resident(RegistryError::Compile(CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }))
}

fn admitted(instruction: &[u8], destination: EffectiveAddress, source: Value32) {
    let mut code = vec![0x90];
    code.extend_from_slice(instruction);
    code.extend_from_slice(&[0xeb, 0]);
    let (mut engine, keep) = fixture(&code);
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE + 1))
            .unwrap()
            .operation(),
        &Operation::Move {
            destination: Location32::Memory(destination),
            source
        }
    );
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), code.len())
        .unwrap();
    describe(&mut engine, &[(CODE, code.len())]);
    let before = saved(&engine, &[keep]);
    let id = engine.compile_resident(1).unwrap().get();
    assert_eq!(saved(&engine, &[keep]), before);
    let bytes = engine.resident_bytes(id).unwrap().to_vec();
    let pointer = engine.resident_bytes(id).unwrap().as_ptr();
    assert_eq!(engine.lookup_resident(CODE + 1).unwrap().get(), id);
    assert_eq!(
        engine
            .lookup_resident(CODE + 1 + instruction.len() as u32)
            .unwrap()
            .get(),
        id
    );
    assert!(
        bytes
            .windows(16)
            .any(|window| window == b"store_resident32")
    );
    assert!(!bytes.windows(7).any(|window| window == b"store32"));
    assert!(!bytes.windows(6).any(|window| window == b"read32"));
    let memory = engine.memory().unwrap();
    for address in [0, DATA, 0x8000, u32::MAX] {
        assert!(memory.resolve(GuestAddress(address), Access::Read).is_err());
        assert!(
            memory
                .resolve(GuestAddress(address), Access::Write)
                .is_err()
        );
    }
    let specs = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: code.len() as u32,
    }];
    let limits = CompileLimits::default();
    let expected = CompileError::Instruction {
        pc: GuestAddress(CODE + 1),
        cause: InstructionError::BackendUnsupported,
    };
    assert_eq!(prepare_region(memory, &specs, limits).err(), Some(expected));
    assert_eq!(compile_region(memory, &specs, limits).err(), Some(expected));
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
        registry.compile(memory, &specs, limits),
        Err(RegistryError::Compile(expected))
    );
    assert_eq!(registry.usage(), usage);
    assert!(memory.is_code_current(&snapshot));
    assert_eq!(ram(&engine, CODE, code.len()), code);
    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile(1), Ok(2));
    assert_eq!(engine.arena(), arena);
    engine.arena_mut().unwrap()[140..].fill(0xa5);
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&CODE.to_le_bytes());
    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile_entries(1, 0), Ok(3));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
    assert_eq!(engine.resident_bytes(id).unwrap().as_ptr(), pointer);
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
}

#[test]
fn three_mov_forms_all_source_registers_and_aliasing_addresses_keep_other_profiles() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    let address = EffectiveAddress {
        base: Some(Ebx),
        index: None,
        scale: 1,
        displacement: 0,
    };
    for (index, register) in [Eax, Ecx, Edx, Ebx, Esp, Ebp, Esi, Edi]
        .into_iter()
        .enumerate()
    {
        admitted(
            &[0x89, 0x03 + (index as u8 * 8)],
            address,
            Value32::Register(register),
        );
    }
    admitted(
        &[0xa3, 0, 0x50, 0, 0],
        EffectiveAddress {
            base: None,
            index: None,
            scale: 1,
            displacement: DATA,
        },
        Value32::Register(Eax),
    );
    admitted(
        &[0xc7, 0x03, 0xef, 0xcd, 0xab, 0x89],
        address,
        Value32::Immediate(0x89ab_cdef),
    );
    for (instruction, base, index, scale, displacement) in [
        (&[0x89, 0x03][..], Some(Ebx), None, 1, 0),
        (&[0x89, 0x04, 0x24], Some(Esp), None, 1, 0),
        (&[0x89, 0x44, 0x24, 0xfc], Some(Esp), None, 1, 0xffff_fffc),
        (&[0x89, 0x45, 0], Some(Ebp), None, 1, 0),
        (&[0x89, 0x44, 0x8b, 0x10], Some(Ebx), Some(Ecx), 4, 16),
        (
            &[0x89, 0x04, 0xcd, 0, 0x80, 0, 0],
            None,
            Some(Ecx),
            8,
            0x8000,
        ),
        (&[0x89, 0x05, 0, 0x80, 0, 0], None, None, 1, 0x8000),
        (
            &[0x89, 0x84, 0xf4, 0, 0, 0, 0x80],
            Some(Esp),
            Some(Esi),
            8,
            0x8000_0000,
        ),
    ] {
        admitted(
            instruction,
            EffectiveAddress {
                base,
                index,
                scale,
                displacement,
            },
            Value32::Register(Eax),
        );
    }
}

#[test]
fn accepted_moves_reach_later_poison_but_cut_spans_and_remaining_operations_do_not() {
    let (mut engine, keep) = fixture(&MOV);
    for instruction in [
        &[0x89, 0x03][..],
        &[0xa3, 0, 0x50, 0, 0],
        &[0xc7, 0x03, 1, 0, 0, 0],
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        upload(&mut engine, CODE, &bytes);
        describe(&mut engine, &[(CODE, bytes.len())]);
        let before = saved(&engine, &[keep]);
        assert_eq!(
            engine.compile_resident(1),
            Err(error(
                CODE + 1 + instruction.len() as u32,
                InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged))
            ))
        );
        assert_eq!(saved(&engine, &[keep]), before);
        describe(&mut engine, &[(CODE, instruction.len())]);
        let before = saved(&engine, &[keep]);
        assert_eq!(
            engine.compile_resident(1),
            Err(error(CODE + 1, InstructionError::InvalidBlockEnd))
        );
        assert_eq!(saved(&engine, &[keep]), before);
    }
    for (instruction, cause) in [
        (&[0xff, 0x13][..], InstructionError::InvalidBlockEnd),
        (&[0xe8, 0, 0, 0, 0], InstructionError::InvalidBlockEnd),
        (&[0xc3], InstructionError::InvalidBlockEnd),
        (
            &[0x66, 0x89, 0x03],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x67, 0x89, 0x03],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0x64, 0x89, 0x03],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Segment)),
        ),
        (
            &[0x18, 0xd0],
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        ),
        (
            &[0xf0, 0x89, 0x03],
            InstructionError::Decode(DecodeError::InvalidEncoding),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        upload(&mut engine, CODE, &bytes);
        describe(&mut engine, &[(CODE, bytes.len())]);
        let before = saved(&engine, &[keep]);
        assert_eq!(engine.compile_resident(1), Err(error(CODE + 1, cause)));
        assert_eq!(saved(&engine, &[keep]), before);
    }
}

#[test]
fn store_identity_errors_precede_guest_faults_and_preserve_every_byte() {
    let (mut engine, keep) = fixture(&MOV);
    engine.map(DATA, 1, 3).unwrap();
    upload(&mut engine, DATA, &[0xa5; 16]);
    let a = compile(&mut engine, &[(CODE, MOV.len())]);
    let (mut foreign, _) = fixture(&MOV);
    let foreign_id = compile(&mut foreign, &[(CODE, MOV.len())]);
    for (key, id, expected) in [
        (KEY ^ (1 << 32), a, HostError::InvalidArtifact),
        (KEY ^ 1, 0, HostError::InvalidArtifact),
        (KEY, 0, HostError::Resident(RegistryError::InvalidUnit)),
        (
            KEY,
            u64::MAX,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (
            KEY,
            foreign_id,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
    ] {
        let before = saved(&engine, &[a, keep]);
        let data = ram(&engine, DATA, 16);
        assert_eq!(engine.store_resident32(key, id, u32::MAX, 0), Err(expected));
        assert_eq!(saved(&engine, &[a, keep]), before);
        assert_eq!(ram(&engine, DATA, 16), data);
        assert_eq!(engine.store_resident32(key, id, DATA, 0), Err(expected));
        assert_eq!(saved(&engine, &[a, keep]), before);
        assert_eq!(ram(&engine, DATA, 16), data);
    }
    upload(&mut engine, CODE, &MOV);
    let before = saved(&engine, &[a, keep]);
    let data = ram(&engine, DATA, 16);
    assert_eq!(
        engine.store_resident32(KEY, a, DATA, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(saved(&engine, &[a, keep]), before);
    assert_eq!(ram(&engine, DATA, 16), data);
    engine.close();
    let before = saved(&engine, &[a, keep]);
    assert_eq!(
        engine.store_resident32(0, 0, u32::MAX, 0),
        Err(HostError::Closed)
    );
    assert_eq!(saved(&engine, &[a, keep]), before);
}

#[test]
fn data_writes_and_atomic_fault_repairs_keep_executing_and_legacy_code_current() {
    let (mut engine, keep) = fixture(&MOV);
    engine.map(DATA, 2, 3).unwrap();
    upload(&mut engine, DATA + 0xffc, &[0xa5; 12]);
    let a = compile(&mut engine, &[(CODE, MOV.len())]);
    let unit = engine.resident_bytes(a).unwrap().to_vec();
    let pointer = engine.resident_bytes(a).unwrap().as_ptr();
    let legacy = engine.artifact_bytes().unwrap().to_vec();
    store(
        &mut engine,
        a,
        DATA + 0xfff,
        0x1234_5678,
        StoreCompletion::Complete,
        [0; 6],
    );
    assert_eq!(
        ram(&engine, DATA + 0xffc, 12),
        [
            0xa5, 0xa5, 0xa5, 0x78, 0x56, 0x34, 0x12, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5
        ]
    );
    engine.protect(DATA + 0x1000, 1, 1).unwrap();
    let before = ram(&engine, DATA + 0xffc, 12);
    store(
        &mut engine,
        a,
        DATA + 0xfff,
        u32::MAX,
        StoreCompletion::Complete,
        [1, 0, 2, DATA + 0x1000, 2, 4],
    );
    assert_eq!(ram(&engine, DATA + 0xffc, 12), before);
    engine.protect(DATA + 0x1000, 1, 3).unwrap();
    store(
        &mut engine,
        a,
        DATA + 0xfff,
        0x7654_3210,
        StoreCompletion::Complete,
        [0; 6],
    );
    assert_eq!(ram(&engine, DATA + 0xfff, 4), 0x7654_3210_u32.to_le_bytes());
    engine.unmap(DATA + 0x1000, 1).unwrap();
    let before = ram(&engine, DATA + 0xffc, 4);
    store(
        &mut engine,
        a,
        DATA + 0xfff,
        u32::MAX,
        StoreCompletion::Complete,
        [1, 0, 1, DATA + 0x1000, 2, 4],
    );
    assert_eq!(ram(&engine, DATA + 0xffc, 4), before);
    engine.map(DATA + 0x1000, 1, 3).unwrap();
    store(
        &mut engine,
        a,
        DATA + 0xfff,
        0x89ab_cdef,
        StoreCompletion::Complete,
        [0; 6],
    );
    assert_eq!(ram(&engine, DATA + 0xfff, 4), 0x89ab_cdef_u32.to_le_bytes());
    store(
        &mut engine,
        a,
        0x8000,
        0,
        StoreCompletion::Complete,
        [1, 0, 1, 0x8000, 2, 4],
    );
    engine.protect(DATA, 1, 1).unwrap();
    let before = ram(&engine, DATA, 16);
    store(
        &mut engine,
        a,
        DATA,
        u32::MAX,
        StoreCompletion::Complete,
        [1, 0, 2, DATA, 2, 4],
    );
    assert_eq!(ram(&engine, DATA, 16), before);
    engine.protect(DATA, 1, 2).unwrap();
    store(
        &mut engine,
        a,
        DATA + 1,
        0xdead_beef,
        StoreCompletion::Complete,
        [0; 6],
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(DATA), &mut [0; 8])
            .is_err()
    );
    engine.protect(DATA, 1, 3).unwrap();
    assert_eq!(ram(&engine, DATA, 8), [0, 0xef, 0xbe, 0xad, 0xde, 0, 0, 0]);
    engine.map(0xffff_f000, 1, 3).unwrap();
    store(
        &mut engine,
        a,
        0xffff_fffc,
        0x4433_2211,
        StoreCompletion::Complete,
        [0; 6],
    );
    let before = ram(&engine, 0xffff_fffc, 4);
    store(
        &mut engine,
        a,
        0xffff_fffd,
        u32::MAX,
        StoreCompletion::Complete,
        [1, 0, 3, 0xffff_fffd, 2, 4],
    );
    assert_eq!(ram(&engine, 0xffff_fffc, 4), before);
    assert_eq!(engine.resident_bytes(a).unwrap(), unit);
    assert_eq!(engine.resident_bytes(a).unwrap().as_ptr(), pointer);
    assert_eq!(engine.artifact_bytes().unwrap(), legacy);
    assert_eq!(engine.guard_resident(KEY, a), Ok(()));
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
}

#[test]
fn successful_code_writes_invalidate_only_the_executing_units_whole_snapshot() {
    for target in [CODE, KEEP, LEGACY] {
        let (mut engine, keep) = fixture(&MOV);
        let a = compile(&mut engine, &[(CODE, MOV.len())]);
        let copied = engine.resident_bytes(a).unwrap().to_vec();
        let pointer = engine.resident_bytes(a).unwrap().as_ptr();
        let word = u32::from_le_bytes(ram(&engine, target, 4).try_into().unwrap());
        let completion = if target == CODE {
            StoreCompletion::CodeInvalidated
        } else {
            StoreCompletion::Complete
        };
        store(&mut engine, a, target, word, completion, [0; 6]);
        assert_eq!(ram(&engine, target, 4), word.to_le_bytes());
        if target == CODE {
            assert_eq!(
                engine.guard_resident(KEY, a),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(
                engine.resident_bytes(a),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            let fresh = compile(&mut engine, &[(CODE, MOV.len())]);
            assert_ne!(fresh, a);
            assert_eq!(engine.lookup_resident(CODE).unwrap().get(), fresh);
            assert_eq!(
                engine.guard_resident(KEY, a),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert!(!engine.resident_bytes(fresh).unwrap().is_empty());
            assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
            assert_eq!(engine.guard(KEY, 1), Ok(()));
        } else {
            assert_eq!(engine.resident_bytes(a).unwrap(), copied);
            assert_eq!(engine.resident_bytes(a).unwrap().as_ptr(), pointer);
            assert_eq!(engine.guard_resident(KEY, a), Ok(()));
            if target == KEEP {
                assert_eq!(
                    engine.guard_resident(KEY, keep),
                    Err(HostError::Resident(RegistryError::CodeInvalidated))
                );
                assert_eq!(engine.guard(KEY, 1), Ok(()));
            } else {
                assert_eq!(engine.guard(KEY, 1), Err(HostError::CodeInvalidated));
                assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
                engine.map(DATA, 1, 3).unwrap();
                store(&mut engine, a, DATA, 7, StoreCompletion::Complete, [0; 6]);
                assert_eq!(ram(&engine, DATA, 4), 7_u32.to_le_bytes());
            }
        }
    }
    let (mut engine, keep) = fixture(&MOV);
    engine.map(OTHER, 1, 7).unwrap();
    upload(&mut engine, OTHER, &[0x90, 0xeb, 0]);
    let a = compile(&mut engine, &[(CODE, MOV.len()), (OTHER, 3)]);
    let word = u32::from_le_bytes(ram(&engine, OTHER, 4).try_into().unwrap());
    store(
        &mut engine,
        a,
        OTHER,
        word,
        StoreCompletion::CodeInvalidated,
        [0; 6],
    );
    assert_eq!(
        engine.guard_resident(KEY, a),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(
        engine.lookup_resident(CODE),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    assert_eq!(engine.guard(KEY, 1), Ok(()));
}

#[test]
fn shared_code_pages_and_permission_repairs_require_fresh_unit_identity() {
    let (mut engine, keep) = fixture(&MOV);
    upload(&mut engine, CODE + 0x100, &[0x90, 0xeb, 0]);
    let a = compile(&mut engine, &[(CODE, MOV.len())]);
    let b = compile(&mut engine, &[(CODE + 0x100, 3)]);
    let word = u32::from_le_bytes(ram(&engine, CODE + 0x100, 4).try_into().unwrap());
    store(
        &mut engine,
        a,
        CODE + 0x100,
        word,
        StoreCompletion::CodeInvalidated,
        [0; 6],
    );
    for id in [a, b] {
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    let fresh = compile(&mut engine, &[(CODE, MOV.len())]);
    engine.protect(CODE, 1, 5).unwrap();
    assert_eq!(
        engine.guard_resident(KEY, fresh),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    let readonly = compile(&mut engine, &[(CODE, MOV.len())]);
    let before = ram(&engine, CODE, 4);
    store(
        &mut engine,
        readonly,
        CODE,
        0,
        StoreCompletion::Complete,
        [1, 0, 2, CODE, 2, 4],
    );
    assert_eq!(ram(&engine, CODE, 4), before);
    assert_eq!(engine.guard_resident(KEY, readonly), Ok(()));
    engine.protect(CODE, 1, 7).unwrap();
    let before = saved(&engine, &[readonly, keep]);
    assert_eq!(
        engine.store_resident32(KEY, readonly, CODE, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(saved(&engine, &[readonly, keep]), before);
    assert_eq!(ram(&engine, CODE, 4), MOV);
    let fresh = compile(&mut engine, &[(CODE, MOV.len())]);
    assert_ne!(fresh, readonly);
    store(
        &mut engine,
        fresh,
        CODE,
        u32::from_le_bytes(MOV),
        StoreCompletion::CodeInvalidated,
        [0; 6],
    );
}

#[test]
fn resident_store_helper_succeeds_without_any_legacy_artifact() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine.map(DATA, 1, 3).unwrap();
    upload(&mut engine, CODE, &MOV);
    let a = compile(&mut engine, &[(CODE, MOV.len())]);
    store(
        &mut engine,
        a,
        DATA,
        0x89ab_cdef,
        StoreCompletion::Complete,
        [0; 6],
    );
    assert_eq!(ram(&engine, DATA, 4), 0x89ab_cdef_u32.to_le_bytes());
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(engine.guard_resident(KEY, a), Ok(()));
}
