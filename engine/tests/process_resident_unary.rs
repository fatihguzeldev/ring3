use ring3_engine::process::{EngineInstance, HostError};

#[test]
fn resident_dec_memory_admits_without_data_mapping_or_legacy_artifact() {
    let mut engine = EngineInstance::new(2, 0x1020_3040_5060_7080).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&[0xff, 0x0b, 0xeb, 0]);
    engine.upload(0x1000, 4).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&4_u32.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = engine
        .compile_resident(1)
        .expect("resident DEC memory admission");
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
            ir::{EffectiveAddress, Location32, Operation, UnaryKind},
        },
    },
    memory::{Access, GuestAddress},
};

const KEY: u64 = 0x1020_3040_5060_7080;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const LEGACY: u32 = 0x3000;
const OTHER: u32 = 0x4000;
const DATA: u32 = 0x5000;
const FORMS: [(u8, u8, UnaryKind); 4] = [
    (0xff, 0, UnaryKind::Inc),
    (0xff, 1, UnaryKind::Dec),
    (0xf7, 2, UnaryKind::Not),
    (0xf7, 3, UnaryKind::Neg),
];

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
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
    let before = engine.arena().to_vec();
    let id = engine.compile_resident(specs.len() as u32).unwrap().get();
    assert_eq!(engine.arena(), before);
    id
}

fn fixture(bytes: &[u8]) -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for address in [CODE, KEEP, LEGACY] {
        engine.map(address, 1, 7).unwrap();
    }
    upload(&mut engine, CODE, bytes);
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
    pointer: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    units: Vec<SavedUnit>,
}

fn saved(engine: &EngineInstance, ids: &[u64]) -> Saved {
    Saved {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
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

fn error(pc: u32, cause: InstructionError) -> CompileError {
    CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    }
}

fn rejected(
    engine: &mut EngineInstance,
    keep: u64,
    bytes: &[u8],
    length: usize,
    pc: u32,
    cause: InstructionError,
) {
    upload(engine, CODE, bytes);
    describe(engine, &[(CODE, length)]);
    let before = saved(engine, &[keep]);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(error(
            pc, cause
        ))))
    );
    assert_eq!(saved(engine, &[keep]), before);
}

fn code(instruction: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x90];
    bytes.extend_from_slice(instruction);
    bytes.extend_from_slice(&[0xeb, 0]);
    bytes
}

fn admitted(instruction: &[u8], kind: UnaryKind, address: EffectiveAddress) {
    let bytes = code(instruction);
    let (mut engine, keep) = fixture(&bytes);
    engine.protect(CODE, 1, 4).unwrap();
    assert_eq!(
        decode_one(engine.memory().unwrap(), GuestAddress(CODE + 1))
            .unwrap()
            .operation(),
        &Operation::Unary {
            kind,
            destination: Location32::Memory(address)
        }
    );
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(CODE), bytes.len())
        .unwrap();
    describe(&mut engine, &[(CODE, bytes.len())]);
    let before = saved(&engine, &[keep]);
    let id = engine.compile_resident(1).unwrap().get();
    assert_ne!(id, keep);
    assert_eq!(saved(&engine, &[keep]), before);
    assert_eq!(engine.lookup_resident(CODE).unwrap().get(), id);
    assert_eq!(engine.lookup_resident(CODE + 1).unwrap().get(), id);
    assert_eq!(
        engine
            .lookup_resident(CODE + 1 + instruction.len() as u32)
            .unwrap()
            .get(),
        id
    );
    assert_eq!(
        engine.lookup_resident(CODE + 2),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(CODE + 2)
        }))
    );
    let module = engine.resident_bytes(id).unwrap();
    assert!(module.windows(6).any(|window| window == b"read32"));
    assert!(
        module
            .windows(16)
            .any(|window| window == b"store_resident32")
    );
    assert!(!module.windows(7).any(|window| window == b"store32"));
    let mut actual = vec![0; bytes.len()];
    let memory = engine.memory().unwrap();
    memory.fetch(GuestAddress(CODE), &mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert!(memory.is_code_current(&snapshot));
    assert!(memory.resolve(GuestAddress(CODE), Access::Read).is_err());
    for pc in [0, DATA, 0x8000, u32::MAX] {
        assert!(memory.resolve(GuestAddress(pc), Access::Read).is_err());
        assert!(memory.resolve(GuestAddress(pc), Access::Write).is_err());
    }
}

#[test]
fn all_four_decoded_unary_forms_and_old_eas_admit_without_readable_code_or_data() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    for (opcode, extension, kind) in FORMS {
        for (index, base) in [Eax, Ecx, Edx, Ebx, Esp, Ebp, Esi, Edi]
            .into_iter()
            .enumerate()
        {
            let mut instruction = vec![opcode];
            if base == Esp {
                instruction.extend_from_slice(&[0x04 | extension << 3, 0x24]);
            } else if base == Ebp {
                instruction.extend_from_slice(&[0x45 | extension << 3, 0]);
            } else {
                instruction.push(index as u8 | extension << 3);
            }
            admitted(
                &instruction,
                kind,
                EffectiveAddress {
                    base: Some(base),
                    index: None,
                    scale: 1,
                    displacement: 0,
                },
            );
        }
        for (tail, base, index, scale, displacement) in [
            (&[0x44, 0x24, 0xfc][..], Some(Esp), None, 1, 0xffff_fffc),
            (&[0x44, 0x8b, 0x10], Some(Ebx), Some(Ecx), 4, 16),
            (&[0x04, 0xcd, 0, 0x80, 0, 0], None, Some(Ecx), 8, 0x8000),
            (&[0x05, 0xff, 0xff, 0xff, 0xff], None, None, 1, u32::MAX),
        ] {
            let mut instruction = vec![opcode, tail[0] | extension << 3];
            instruction.extend_from_slice(&tail[1..]);
            admitted(
                &instruction,
                kind,
                EffectiveAddress {
                    base,
                    index,
                    scale,
                    displacement,
                },
            );
        }
    }
}

#[test]
fn resident_profile_keeps_public_cold_unbound_exclusions_and_legacy_equivalence() {
    for (opcode, extension, kind) in FORMS {
        let instruction = [opcode, 0x03 | extension << 3];
        let bytes = code(&instruction);
        let (mut engine, keep) = fixture(&bytes);
        let id = compile(&mut engine, &[(CODE, bytes.len())]);
        let module = engine.resident_bytes(id).unwrap().to_vec();
        let pointer = engine.resident_bytes(id).unwrap().as_ptr();
        let before = saved(&engine, &[id, keep]);
        let memory = engine.memory().unwrap();
        let specs = [BlockSpec {
            entry: GuestAddress(CODE),
            byte_length: bytes.len() as u32,
        }];
        let expected = error(CODE + 1, InstructionError::BackendUnsupported);
        let limits = CompileLimits::default();
        assert_eq!(
            prepare_region(memory, &specs, limits).err(),
            Some(expected),
            "{kind:?}"
        );
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
        assert_eq!(saved(&engine, &[id, keep]), before);
        let arena = engine.arena().to_vec();
        assert_eq!(engine.compile(1), Ok(2));
        assert_eq!(engine.arena(), arena);
        engine.arena_mut().unwrap()[140..].fill(0xa5);
        engine.arena_mut().unwrap()[140..144].copy_from_slice(&CODE.to_le_bytes());
        let arena = engine.arena().to_vec();
        assert_eq!(engine.compile_entries(1, 0), Ok(3));
        assert_eq!(engine.arena(), arena);
        assert_eq!(engine.resident_bytes(id).unwrap(), module);
        assert_eq!(engine.resident_bytes(id).unwrap().as_ptr(), pointer);
        assert_eq!(engine.guard_resident(KEY, id), Ok(()));
        assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    }
}

#[test]
fn span_poison_and_remaining_exclusions_preserve_arena_and_installed_units() {
    let (mut engine, keep) = fixture(&[0x90, 0xeb, 0]);
    for (opcode, extension, _) in FORMS {
        let instruction = [opcode, 0x03 | extension << 3];
        let bytes = [0x90, instruction[0], instruction[1], 0x0f, 0x06];
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            CODE + 3,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Privileged)),
        );
        rejected(
            &mut engine,
            keep,
            &bytes,
            2,
            CODE + 1,
            InstructionError::InvalidBlockEnd,
        );
        for (prefix, feature) in [
            (0x66, UnsupportedFeature::Opcode),
            (0x67, UnsupportedFeature::Opcode),
            (0xf0, UnsupportedFeature::Opcode),
            (0x64, UnsupportedFeature::Segment),
        ] {
            let bytes = [0x90, prefix, instruction[0], instruction[1], 0x0f, 0x06];
            rejected(
                &mut engine,
                keep,
                &bytes,
                bytes.len(),
                CODE + 1,
                InstructionError::Decode(DecodeError::Unsupported(feature)),
            );
        }
        let narrow = if opcode == 0xff { 0xfe } else { 0xf6 };
        let bytes = [0x90, narrow, instruction[1], 0x0f, 0x06];
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            CODE + 1,
            InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
        );
    }
    for instruction in [&[0xe8, 0, 0, 0, 0][..], &[0xff, 0x13], &[0xc3]] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0x0f, 0x06]);
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            CODE + 1,
            InstructionError::InvalidBlockEnd,
        );
    }
    describe(&mut engine, &[(CODE, 3)]);
    let before = saved(&engine, &[keep]);
    assert_eq!(engine.compile_resident(0), Err(HostError::InvalidRequest));
    assert_eq!(engine.compile_resident(9), Err(HostError::InvalidRequest));
    assert_eq!(saved(&engine, &[keep]), before);
}

#[test]
fn unary_units_ignore_live_data_and_keep_whole_unit_code_snapshots_and_fresh_ids() {
    for (opcode, extension, _) in FORMS {
        let bytes = [opcode, 0x03 | extension << 3, 0xeb, 0];
        let (mut engine, keep) = fixture(&bytes);
        engine.map(OTHER, 1, 7).unwrap();
        upload(&mut engine, OTHER, &[0x90, 0xeb, 0]);
        let id = compile(&mut engine, &[(CODE, bytes.len()), (OTHER, 3)]);
        let module = engine.resident_bytes(id).unwrap().to_vec();
        let pointer = engine.resident_bytes(id).unwrap().as_ptr();
        engine.map(DATA, 1, 3).unwrap();
        for value in [0, u32::MAX, 0x8000_0000] {
            let before = saved(&engine, &[id, keep]);
            engine.write32(DATA, value).unwrap();
            assert_eq!(saved(&engine, &[id, keep]), before);
        }
        let before = saved(&engine, &[id, keep]);
        engine.protect(DATA, 1, 2).unwrap();
        engine.unmap(DATA, 1).unwrap();
        engine.map(DATA, 1, 3).unwrap();
        assert_eq!(saved(&engine, &[id, keep]), before);
        assert_eq!(engine.resident_bytes(id).unwrap(), module);
        assert_eq!(engine.resident_bytes(id).unwrap().as_ptr(), pointer);
        let before = engine.arena().to_vec();
        engine.write32(OTHER, 0x0000_eb90).unwrap();
        assert_eq!(engine.arena(), before);
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            engine.lookup_resident(CODE),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
        assert_eq!(engine.guard(KEY, 1), Ok(()));
        let fresh = compile(&mut engine, &[(CODE, bytes.len()), (OTHER, 3)]);
        assert_ne!(fresh, id);
        assert_eq!(engine.lookup_resident(CODE).unwrap().get(), fresh);
        assert_eq!(engine.lookup_resident(OTHER).unwrap().get(), fresh);
        assert_eq!(engine.guard_resident(KEY, fresh), Ok(()));
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
}
