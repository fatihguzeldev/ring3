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
const LEGACY: u32 = 0x3000;
const OTHER: u32 = 0x4000;
const DATA: u32 = 0x5000;
const EBX_ADDRESS: EffectiveAddress = EffectiveAddress {
    base: Some(Register32::Ebx),
    index: None,
    scale: 1,
    displacement: 0,
};
const FORMS: [(&[u8], Operation); 5] = [
    (
        &[0xe8, 0, 0x10, 0, 0],
        Operation::Call {
            target: BranchTarget::Direct(GuestAddress(CODE + 6 + 0x1000)),
        },
    ),
    (
        &[0xff, 0xd0],
        Operation::Call {
            target: BranchTarget::Indirect(Location32::Register(Register32::Eax)),
        },
    ),
    (
        &[0xff, 0x13],
        Operation::Call {
            target: BranchTarget::Indirect(Location32::Memory(EBX_ADDRESS)),
        },
    ),
    (&[0xc3], Operation::Return { stack_adjust: 0 }),
    (&[0xc2, 0, 0], Operation::Return { stack_adjust: 0 }),
];

#[test]
fn resident_call_esp_admits_without_target_or_stack_data() {
    let mut engine = EngineInstance::new(2, 0x1020_3040_5060_7080).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[0xff, 0xd4]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&2_u32.to_le_bytes());
    let before = engine.arena().to_vec();
    let id = engine
        .compile_resident(1)
        .expect("resident CALL ESP admission");
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(engine.lookup_resident(0x1000), Ok(id));
    assert_eq!(
        engine.guard_resident(0x1020_3040_5060_7080, id.get()),
        Ok(())
    );
}

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
    bytes
}

fn admitted(instruction: &[u8], operation: Operation) {
    let bytes = code(instruction);
    let (mut engine, keep) = fixture(&bytes);
    engine.protect(CODE, 1, 4).unwrap();
    assert_eq!(
        *decode_one(engine.memory().unwrap(), GuestAddress(CODE + 1))
            .unwrap()
            .operation(),
        operation
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
    for pc in [CODE, CODE + 1] {
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
    }
    if instruction.len() > 1 {
        assert_eq!(
            engine.lookup_resident(CODE + 2),
            Err(HostError::Resident(RegistryError::NotFound {
                pc: GuestAddress(CODE + 2)
            }))
        );
    }
    assert_eq!(
        engine.lookup_resident(CODE + bytes.len() as u32),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(CODE + bytes.len() as u32)
        }))
    );
    let reads = matches!(
        operation,
        Operation::Call {
            target: BranchTarget::Indirect(Location32::Memory(_))
        } | Operation::Return { .. }
    );
    let stores = matches!(operation, Operation::Call { .. });
    let module = engine.resident_bytes(id).unwrap();
    assert_eq!(module.windows(6).any(|window| window == b"read32"), reads);
    assert_eq!(
        module
            .windows(16)
            .any(|window| window == b"store_resident32"),
        stores
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
fn all_five_families_register_targets_memory_eas_and_unsigned_cleanup_admit_without_data() {
    use Register32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};
    for displacement in [0_u32, 0x7fff_ffff, 0x8000_0000, 0xffff_ff7f, u32::MAX] {
        let mut instruction = vec![0xe8];
        instruction.extend_from_slice(&displacement.to_le_bytes());
        admitted(
            &instruction,
            Operation::Call {
                target: BranchTarget::Direct(GuestAddress((CODE + 6).wrapping_add(displacement))),
            },
        );
    }
    for (index, register) in [Eax, Ecx, Edx, Ebx, Esp, Ebp, Esi, Edi]
        .into_iter()
        .enumerate()
    {
        admitted(
            &[0xff, 0xd0 + index as u8],
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Register(register)),
            },
        );
        let mut instruction = vec![0xff];
        if register == Esp {
            instruction.extend_from_slice(&[0x14, 0x24]);
        } else if register == Ebp {
            instruction.extend_from_slice(&[0x55, 0]);
        } else {
            instruction.push(0x10 | index as u8);
        }
        admitted(
            &instruction,
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Memory(EffectiveAddress {
                    base: Some(register),
                    index: None,
                    scale: 1,
                    displacement: 0,
                })),
            },
        );
    }
    for (tail, base, index, scale, displacement) in [
        (&[0x54, 0x24, 0xfc][..], Some(Esp), None, 1, 0xffff_fffc),
        (&[0x54, 0x24, 0xfe], Some(Esp), None, 1, 0xffff_fffe),
        (&[0x54, 0x8c, 8], Some(Esp), Some(Ecx), 4, 8),
        (&[0x54, 0x4b, 0xfc], Some(Ebx), Some(Ecx), 2, 0xffff_fffc),
        (&[0x14, 0xcd, 0, 0x80, 0, 0], None, Some(Ecx), 8, 0x8000),
        (&[0x15, 0xff, 0xff, 0xff, 0xff], None, None, 1, u32::MAX),
        (
            &[0x94, 0xf4, 0, 0, 0, 0x80],
            Some(Esp),
            Some(Esi),
            8,
            0x8000_0000,
        ),
    ] {
        let mut instruction = vec![0xff];
        instruction.extend_from_slice(tail);
        admitted(
            &instruction,
            Operation::Call {
                target: BranchTarget::Indirect(Location32::Memory(EffectiveAddress {
                    base,
                    index,
                    scale,
                    displacement,
                })),
            },
        );
    }
    admitted(&[0xc3], Operation::Return { stack_adjust: 0 });
    for stack_adjust in [0_u16, 1, 4, u16::MAX] {
        let mut instruction = vec![0xc2];
        instruction.extend_from_slice(&stack_adjust.to_le_bytes());
        admitted(&instruction, Operation::Return { stack_adjust });
    }
}

#[test]
fn resident_profile_keeps_public_cold_unbound_exclusions_and_legacy_equivalence() {
    for (instruction, operation) in FORMS {
        let bytes = code(instruction);
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
            "{operation:?}"
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
fn near_control_end_prefix_and_decode_errors_preserve_arena_units_and_capacity() {
    let (mut engine, keep) = fixture(&[0x90, 0xeb, 0]);
    for (instruction, _) in FORMS {
        for suffix in [&[0x90][..], &[0x0f, 0x06]] {
            let mut bytes = code(instruction);
            bytes.extend_from_slice(suffix);
            rejected(
                &mut engine,
                keep,
                &bytes,
                bytes.len(),
                CODE + 1,
                InstructionError::InvalidBlockEnd,
            );
        }
        let bytes = code(instruction);
        if instruction.len() > 1 {
            rejected(
                &mut engine,
                keep,
                &bytes,
                instruction.len(),
                CODE + 1,
                InstructionError::InvalidBlockEnd,
            );
        } else {
            describe(&mut engine, &[(CODE, 0)]);
            let before = saved(&engine, &[keep]);
            assert_eq!(
                engine.compile_resident(1),
                Err(HostError::Resident(RegistryError::Compile(
                    CompileError::InvalidBlocks
                )))
            );
            assert_eq!(saved(&engine, &[keep]), before);
        }
        for (prefix, cause) in [
            (0x66, DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            (0x67, DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            (0x64, DecodeError::Unsupported(UnsupportedFeature::Segment)),
            (0xf0, DecodeError::InvalidEncoding),
        ] {
            let mut bytes = vec![0x90, prefix];
            bytes.extend_from_slice(instruction);
            bytes.extend_from_slice(&[0x0f, 0x06]);
            rejected(
                &mut engine,
                keep,
                &bytes,
                bytes.len(),
                CODE + 1,
                InstructionError::Decode(cause),
            );
        }
    }
    for (instruction, cause) in [
        (
            &[0xff, 0x1b][..],
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            &[0xcb],
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            &[0xca, 8, 0],
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            &[0xc0, 0x20, 2],
            DecodeError::Unsupported(UnsupportedFeature::Opcode),
        ),
        (
            &[0x0f, 0x06],
            DecodeError::Unsupported(UnsupportedFeature::Privileged),
        ),
    ] {
        let mut bytes = vec![0x90];
        bytes.extend_from_slice(instruction);
        bytes.extend_from_slice(&[0xc3]);
        rejected(
            &mut engine,
            keep,
            &bytes,
            bytes.len(),
            CODE + 1,
            InstructionError::Decode(cause),
        );
    }
    describe(&mut engine, &[(CODE, 3)]);
    let before = saved(&engine, &[keep]);
    for count in [0, 9, u32::MAX] {
        assert_eq!(
            engine.compile_resident(count),
            Err(HostError::InvalidRequest)
        );
    }
    assert_eq!(saved(&engine, &[keep]), before);
    let mut bytes = vec![0x90; 7 * 16];
    for index in 0..7 {
        bytes[index * 16..index * 16 + 3].copy_from_slice(&[0x90, 0xeb, 0]);
    }
    upload(&mut engine, CODE, &bytes);
    let mut ids = vec![keep];
    for index in 0..7 {
        describe(&mut engine, &[(CODE + index * 16, 3)]);
        let before = saved(&engine, &ids);
        let id = engine.compile_resident(1).unwrap().get();
        assert_ne!(id, 0);
        assert!(!ids.contains(&id));
        assert_eq!(saved(&engine, &ids), before);
        ids.push(id);
    }
    let before = saved(&engine, &ids);
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::UnitCapacity))
    );
    assert_eq!(saved(&engine, &ids), before);
}

#[test]
fn near_control_units_ignore_live_data_and_keep_whole_unit_code_snapshots_and_fresh_ids() {
    for (instruction, _) in FORMS {
        let bytes = instruction.to_vec();
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
