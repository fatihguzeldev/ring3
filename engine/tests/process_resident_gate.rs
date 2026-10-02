use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{
        ExecutionExit, ExitReason, UnsupportedFeature,
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, RegistryError,
            RegistryLimits, ResidentRegistry, compile_entry_region, compile_region,
            prepare_entry_region, prepare_region,
        },
        x86::{State32, decode::DecodeError},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x9345_6789_a000_0001;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const LEGACY: u32 = 0x4000;
const OTHER: u32 = 0x5000;

#[test]
fn resident_gate_admits_execute_only_marker_without_data_stack_or_legacy() {
    let mut engine = EngineInstance::new(1, 0x9345_6789_a000_0001).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    engine.protect(0x1000, 1, 4).unwrap();
    for (index, value) in [0x1000_u32, 2, 0x1000, 7].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(engine.generation(), 0);
    let id = engine.compile_resident_with_gates(1, 1).unwrap();
    assert_eq!(engine.lookup_resident(0x1000), Ok(id));
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..];
    transfer.fill(0xa5);
    for (index, &(first, second)) in blocks.iter().chain(gates).enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&first.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&second.to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    describe(engine, blocks, gates);
    let before = engine.arena().to_vec();
    let id = engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get();
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
    id
}

fn fixture() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(10, KEY).unwrap();
    for pc in [CODE, KEEP, LEGACY, OTHER] {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, CODE, &[0x0f, 0x0b]);
    for pc in [KEEP, LEGACY, OTHER] {
        upload(&mut engine, pc, &[0x90, 0xeb, 0]);
    }
    describe(&mut engine, &[(LEGACY, 3)], &[]);
    assert_eq!(engine.compile(1), Ok(1));
    let keep = compile(&mut engine, &[(KEEP, 3)], &[]);
    (engine, keep)
}

#[derive(Debug, PartialEq, Eq)]
struct SavedUnit {
    id: u64,
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    legacy: Result<Vec<u8>, HostError>,
    units: Vec<SavedUnit>,
    lookup: Vec<Result<u64, HostError>>,
}

fn saved(engine: &EngineInstance, ids: &[u64], pcs: &[u32]) -> Saved {
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
            })
            .collect(),
        lookup: pcs
            .iter()
            .map(|&pc| engine.lookup_resident(pc).map(|id| id.get()))
            .collect(),
    }
}

fn reject<T: std::fmt::Debug + PartialEq>(
    engine: &mut EngineInstance,
    ids: &[u64],
    expected: HostError,
    action: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = saved(engine, ids, &[CODE, KEEP, OTHER]);
    assert_eq!(action(engine), Err(expected));
    assert_eq!(saved(engine, ids, &[CODE, KEEP, OTHER]), before);
}

fn compile_error(error: CompileError) -> HostError {
    HostError::Resident(RegistryError::Compile(error))
}

fn instruction_error(pc: u32, cause: InstructionError) -> HostError {
    compile_error(CompileError::Instruction {
        pc: GuestAddress(pc),
        cause,
    })
}

fn opcode_error(pc: u32) -> HostError {
    instruction_error(
        pc,
        InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    )
}

fn fetch_error(pc: u32, address: u32, reason: FaultReason) -> HostError {
    instruction_error(
        pc,
        InstructionError::Decode(DecodeError::MemoryFault {
            pc: GuestAddress(pc),
            fault: MemoryFault {
                address: GuestAddress(address),
                access: Access::Execute,
                reason,
            },
            length: 2,
        }),
    )
}

#[test]
fn descriptors_markers_fetch_and_profiles_preserve_arena_legacy_and_retained_units() {
    let (mut engine, keep) = fixture();
    for (blocks, gates) in [(0, 0), (9, 0), (1, 2), (8, 9), (u32::MAX, 0), (1, u32::MAX)] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..].fill(0xff);
        reject(&mut engine, &[keep], HostError::InvalidRequest, |engine| {
            engine.compile_resident_with_gates(blocks, gates)
        });
    }
    for blocks in [
        vec![(CODE, 0)],
        vec![(u32::MAX, 2)],
        vec![(CODE, 2), (CODE + 1, 2)],
        vec![(CODE, 2), (CODE, 2)],
    ] {
        describe(&mut engine, &blocks, &[(blocks[0].0, 0)]);
        reject(
            &mut engine,
            &[keep],
            compile_error(CompileError::InvalidBlocks),
            |engine| engine.compile_resident_with_gates(blocks.len() as u32, 1),
        );
    }
    for (blocks, gates) in [
        (vec![(CODE, 2)], vec![(CODE, 0)]),
        (vec![(CODE, 2)], vec![(OTHER + 0x1000, 7)]),
        (vec![(CODE, 1)], vec![(CODE, 7)]),
        (vec![(CODE, 3)], vec![(CODE, 7)]),
        (vec![(CODE, 3)], vec![(CODE + 1, 7)]),
        (vec![(CODE, 2), (OTHER, 2)], vec![(CODE, 7), (CODE, 8)]),
        (vec![(CODE, 2), (OTHER, 2)], vec![(CODE, 7), (OTHER, 7)]),
    ] {
        describe(&mut engine, &blocks, &gates);
        reject(
            &mut engine,
            &[keep],
            compile_error(CompileError::InvalidGates),
            |engine| engine.compile_resident_with_gates(blocks.len() as u32, gates.len() as u32),
        );
    }
    for marker in [[0x90, 0x90], [0x0b, 0x0f], [0x66, 0x0f], [0x0f, 0x0a]] {
        upload(&mut engine, CODE, &marker);
        describe(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
        reject(
            &mut engine,
            &[keep],
            instruction_error(CODE, InstructionError::InvalidGate),
            |engine| engine.compile_resident_with_gates(1, 1),
        );
    }
    for marker in [&[0x0f, 0x0b][..], &[0x66, 0x0f, 0x0b][..]] {
        upload(&mut engine, CODE, marker);
        describe(&mut engine, &[(CODE, marker.len() as u32)], &[]);
        reject(&mut engine, &[keep], opcode_error(CODE), |engine| {
            engine.compile_resident_with_gates(1, 0)
        });
    }
    upload(&mut engine, CODE, &[0x0f, 0x0b]);
    let spec = [BlockSpec {
        entry: GuestAddress(CODE),
        byte_length: 2,
    }];
    let limits = CompileLimits::default();
    let memory = engine.memory().unwrap();
    let expected = CompileError::Instruction {
        pc: GuestAddress(CODE),
        cause: InstructionError::Decode(DecodeError::Unsupported(UnsupportedFeature::Opcode)),
    };
    assert_eq!(
        prepare_region(memory, &spec, limits).map(|_| ()),
        Err(expected)
    );
    assert_eq!(
        compile_region(memory, &spec, limits).map(|_| ()),
        Err(expected)
    );
    assert_eq!(
        prepare_entry_region(memory, &[GuestAddress(CODE)], limits).map(|_| ()),
        Err(expected)
    );
    assert_eq!(
        compile_entry_region(memory, &[GuestAddress(CODE)], limits).map(|_| ()),
        Err(expected)
    );
    let mut unbound = ResidentRegistry::new(memory, RegistryLimits::default()).unwrap();
    assert_eq!(
        unbound.compile(memory, &spec, limits),
        Err(RegistryError::Compile(expected))
    );
    assert_eq!(unbound.usage().units, 0);
    assert_eq!(unbound.usage().wasm_bytes, 0);
    let gate = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    describe(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    let before = saved(&engine, &[keep, gate], &[CODE, KEEP]);
    assert_eq!(engine.compile_with_gates(1, 1), Ok(2));
    assert_eq!(engine.arena(), before.arena);
    assert_eq!(
        engine.resident_bytes(gate).map(|bytes| bytes.to_vec()),
        before.units[1].bytes
    );
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    describe(&mut engine, &[(OTHER, 3)], &[(OTHER, 0)]);
    let before = engine.arena().to_vec();
    let ordinary = engine.compile_resident(1).unwrap().get();
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.lookup_resident(OTHER).map(|id| id.get()),
        Ok(ordinary)
    );
    assert_eq!(engine.guard_resident(KEY, gate), Ok(()));

    for (pc, denied, reason, second_page) in [
        (0x7000, 0x7000, FaultReason::Unmapped, false),
        (CODE, CODE, FaultReason::Permission, false),
        (0x1fff, 0x2000, FaultReason::Unmapped, false),
        (0x1fff, 0x2000, FaultReason::Permission, true),
    ] {
        let (mut engine, keep) = fixture();
        if pc == CODE {
            engine.protect(CODE, 1, 1).unwrap();
        }
        if pc == 0x1fff {
            upload(&mut engine, pc, &[0x0f]);
            if second_page {
                engine.map(0x2000, 1, 7).unwrap();
                upload(&mut engine, 0x2000, &[0x0b]);
                engine.protect(0x2000, 1, 1).unwrap();
            }
        }
        describe(&mut engine, &[(pc, 2)], &[(pc, 7)]);
        reject(
            &mut engine,
            &[keep],
            fetch_error(pc, denied, reason),
            |engine| engine.compile_resident_with_gates(1, 1),
        );
    }
    for index in 0..5 {
        upload(&mut engine, OTHER + 0x100 + index * 16, &[0x90, 0xeb, 0]);
    }
    let mut retained = vec![keep, gate, ordinary];
    for index in 0..5 {
        retained.push(compile(
            &mut engine,
            &[(OTHER + 0x100 + index * 16, 3)],
            &[],
        ));
    }
    describe(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    reject(
        &mut engine,
        &retained,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident_with_gates(1, 1),
    );
    engine.close();
    reject(
        &mut engine,
        &[keep, gate, ordinary],
        HostError::Closed,
        |engine| engine.compile_resident_with_gates(0, u32::MAX),
    );
}

#[test]
fn eight_descriptors_crosspage_markers_and_gate_instruction_charges_keep_exact_bounds() {
    let (mut engine, keep) = fixture();
    let ids = [1, 2, 3, 4, 5, 6, 0x8000_0000, u32::MAX];
    let blocks: Vec<_> = (0..8).map(|i| (CODE + i * 16, 2)).collect();
    for &(pc, _) in &blocks {
        upload(&mut engine, pc, &[0x0f, 0x0b]);
    }
    let gates: Vec<_> = blocks
        .iter()
        .zip(ids)
        .map(|(&(pc, _), id)| (pc, id))
        .collect();
    let before_legacy = engine.artifact_bytes().unwrap().to_vec();
    let unit = compile(&mut engine, &blocks, &gates);
    assert_eq!(engine.artifact_bytes().unwrap(), before_legacy);
    for &(pc, _) in &blocks {
        assert_eq!(engine.lookup_resident(pc).map(|id| id.get()), Ok(unit));
        for missing in [pc + 1, pc + 2] {
            assert_eq!(
                engine.lookup_resident(missing),
                Err(HostError::Resident(RegistryError::NotFound {
                    pc: GuestAddress(missing)
                }))
            );
        }
    }
    let bytes = engine.resident_bytes(unit).unwrap();
    assert!(
        bytes
            .windows(b"guard_resident".len())
            .any(|part| part == b"guard_resident")
    );
    for absent in [b"read32".as_slice(), b"store_resident32".as_slice()] {
        assert!(!bytes.windows(absent.len()).any(|part| part == absent));
    }
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));

    for pc in [0x1fff, u32::MAX - 1] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        let page = pc & !0xfff;
        let pages = if pc == 0x1fff { 2 } else { 1 };
        engine.map(page, pages, 7).unwrap();
        upload(&mut engine, pc, &[0x0f, 0x0b]);
        engine.protect(page, pages, 4).unwrap();
        let id = compile(&mut engine, &[(pc, 2)], &[(pc, u32::MAX)]);
        assert_eq!(engine.lookup_resident(pc).map(|id| id.get()), Ok(id));
        assert_eq!(engine.generation(), 0);
    }

    let (mut engine, keep) = fixture();
    engine.map(0x7000, 1, 7).unwrap();
    let mut exactly_63 = vec![0x90; 62];
    exactly_63.extend([0xeb, 0]);
    upload(&mut engine, OTHER, &exactly_63);
    let mut limit_before_poison = vec![0x90; 63];
    limit_before_poison.extend([0x0f, 0x06]);
    upload(&mut engine, 0x7000, &limit_before_poison);
    let mut exactly_64 = vec![0x90; 63];
    exactly_64.extend([0xeb, 0]);
    upload(&mut engine, 0x7100, &exactly_64);
    upload(&mut engine, CODE + 16, &[0x90, 0x90]);
    let full = compile(
        &mut engine,
        &[(CODE, 2), (OTHER, exactly_63.len() as u32)],
        &[(CODE, 7)],
    );
    assert_eq!(
        engine.lookup_resident(OTHER + 61).map(|id| id.get()),
        Ok(full)
    );
    assert_eq!(
        engine.lookup_resident(OTHER + 62).map(|id| id.get()),
        Ok(full)
    );
    for (blocks, gates) in [
        (
            vec![(CODE, 2), (0x7000, limit_before_poison.len() as u32)],
            vec![(CODE, 7)],
        ),
        (
            vec![(0x7100, exactly_64.len() as u32), (CODE + 16, 2)],
            vec![(CODE + 16, 8)],
        ),
    ] {
        describe(&mut engine, &blocks, &gates);
        reject(
            &mut engine,
            &[keep, full],
            compile_error(CompileError::InstructionLimit),
            |engine| engine.compile_resident_with_gates(2, 1),
        );
    }
}

#[test]
fn executable_starts_collide_exactly_and_marker_inside_immediate_keeps_both_orders_valid() {
    let (mut engine, keep) = fixture();
    upload(&mut engine, OTHER, &[0x0f, 0x0b]);
    let gate = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    for id in [7, 8, u32::MAX] {
        describe(&mut engine, &[(CODE, 2)], &[(CODE, id)]);
        reject(
            &mut engine,
            &[keep, gate],
            HostError::Resident(RegistryError::InstructionOverlap {
                pc: GuestAddress(CODE),
            }),
            |engine| engine.compile_resident_with_gates(1, 1),
        );
    }
    describe(&mut engine, &[(KEEP, 3)], &[]);
    reject(
        &mut engine,
        &[keep, gate],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(KEEP),
        }),
        |engine| engine.compile_resident_with_gates(1, 0),
    );
    let same_numeric_id = compile(&mut engine, &[(OTHER, 2)], &[(OTHER, 7)]);
    assert_ne!(same_numeric_id, gate);
    assert_eq!(engine.lookup_resident(CODE).map(|id| id.get()), Ok(gate));
    assert_eq!(
        engine.lookup_resident(OTHER).map(|id| id.get()),
        Ok(same_numeric_id)
    );

    for gate_first in [false, true] {
        let (mut engine, keep) = fixture();
        upload(&mut engine, CODE, &[0xb8, 0x0f, 0x0b, 0x90, 0x90, 0xeb, 0]);
        let (first_blocks, first_gates) = if gate_first {
            (vec![(CODE + 1, 2)], vec![(CODE + 1, 17)])
        } else {
            (vec![(CODE, 7)], vec![])
        };
        let first = compile(&mut engine, &first_blocks, &first_gates);
        let first_bytes = engine.resident_bytes(first).unwrap().to_vec();
        let first_pointer = engine.resident_bytes(first).unwrap().as_ptr();
        let second = if gate_first {
            compile(&mut engine, &[(CODE, 7)], &[])
        } else {
            compile(&mut engine, &[(CODE + 1, 2)], &[(CODE + 1, 17)])
        };
        let (ordinary, gate) = if gate_first {
            (second, first)
        } else {
            (first, second)
        };
        assert_eq!(engine.resident_bytes(first).unwrap(), first_bytes);
        assert_eq!(
            engine.resident_bytes(first).unwrap().as_ptr(),
            first_pointer
        );
        assert_eq!(
            engine.lookup_resident(CODE).map(|id| id.get()),
            Ok(ordinary)
        );
        assert_eq!(
            engine.lookup_resident(CODE + 5).map(|id| id.get()),
            Ok(ordinary)
        );
        assert_eq!(
            engine.lookup_resident(CODE + 1).map(|id| id.get()),
            Ok(gate)
        );
        for missing in [CODE + 2, CODE + 3, CODE + 4, CODE + 6, CODE + 7] {
            assert_eq!(
                engine.lookup_resident(missing),
                Err(HostError::Resident(RegistryError::NotFound {
                    pc: GuestAddress(missing)
                }))
            );
        }
        assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
    }

    let (mut engine, keep) = fixture();
    let gate = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    upload(&mut engine, CODE, &[0x90, 0xeb, 0]);
    let ordinary = compile(&mut engine, &[(CODE, 3)], &[]);
    assert!(ordinary > gate);
    assert_eq!(
        engine.lookup_resident(CODE).map(|id| id.get()),
        Ok(ordinary)
    );
    assert_eq!(
        engine.guard_resident(KEY, gate),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    upload(&mut engine, CODE, &[0x0f, 0x0b]);
    let repaired = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    assert!(repaired > ordinary);
    assert_eq!(
        engine.lookup_resident(CODE).map(|id| id.get()),
        Ok(repaired)
    );
    assert_eq!(
        engine.guard_resident(KEY, ordinary),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
}

#[test]
fn snapshots_cover_both_marker_pages_and_whole_unit_while_stale_entries_retain_capacity() {
    for page in [0x1000, 0x2000] {
        for change in 0..3 {
            let (mut engine, keep) = fixture();
            engine.map(0x2000, 1, 7).unwrap();
            engine.map(0x6000, 1, 3).unwrap();
            upload(&mut engine, 0x1fff, &[0x0f, 0x0b]);
            let old = compile(&mut engine, &[(0x1fff, 2), (OTHER, 3)], &[(0x1fff, 7)]);
            let bytes = engine.resident_bytes(old).unwrap().to_vec();
            let pointer = engine.resident_bytes(old).unwrap().as_ptr();
            engine.write32(0x6000, 0x7654_3210).unwrap();
            engine.protect(0x6000, 1, 1).unwrap();
            engine.unmap(0x6000, 1).unwrap();
            engine.map(0x6000, 1, 3).unwrap();
            assert_eq!(engine.resident_bytes(old).unwrap(), bytes);
            assert_eq!(engine.resident_bytes(old).unwrap().as_ptr(), pointer);
            let pc = if page == CODE { 0x1fff } else { 0x2000 };
            let marker_byte = if page == CODE { 0x0f } else { 0x0b };
            match change {
                0 => upload(&mut engine, pc, &[marker_byte]),
                1 => engine.protect(page, 1, 7).unwrap(),
                _ => {
                    engine.unmap(page, 1).unwrap();
                    engine.map(page, 1, 7).unwrap();
                    upload(&mut engine, pc, &[marker_byte]);
                }
            }
            assert_eq!(
                engine.guard_resident(KEY, old),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(
                engine.resident_bytes(old),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            for pc in [0x1fff, OTHER, OTHER + 1] {
                assert_eq!(
                    engine.lookup_resident(pc),
                    Err(HostError::Resident(RegistryError::CodeInvalidated))
                );
            }
            let fresh = compile(&mut engine, &[(0x1fff, 2), (OTHER, 3)], &[(0x1fff, 7)]);
            assert!(fresh > old);
            assert_eq!(engine.lookup_resident(0x1fff).map(|id| id.get()), Ok(fresh));
            assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
            assert_eq!(engine.guard(KEY, 1), Ok(()));
            upload(&mut engine, OTHER, &[0x90]);
            assert_eq!(
                engine.lookup_resident(0x1fff),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(engine.guard_resident(KEY, keep), Ok(()));
        }
    }
    let (mut engine, keep) = fixture();
    let mut retained = vec![keep];
    for _ in 0..7 {
        upload(&mut engine, CODE, &[0x0f, 0x0b]);
        let id = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
        if let Some(&previous) = retained.last() {
            assert!(id > previous);
        }
        retained.push(id);
    }
    let current = *retained.last().unwrap();
    assert_eq!(engine.lookup_resident(CODE).map(|id| id.get()), Ok(current));
    for &id in &retained[1..retained.len() - 1] {
        assert_eq!(
            engine.guard_resident(KEY, id),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
    }
    describe(&mut engine, &[(CODE, 2)], &[(CODE, 0)]);
    reject(
        &mut engine,
        &retained,
        HostError::InvalidRequest,
        |engine| engine.compile_resident_with_gates(1, 2),
    );
    reject(
        &mut engine,
        &retained,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident_with_gates(1, 1),
    );
}

fn stop_input(engine: &mut EngineInstance, pc: u32, id: u32, esp: u32) {
    let mut state = State32 {
        eip: pc,
        ..State32::default()
    };
    state.registers[4] = esp;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

#[test]
fn legacy_capture_ceiling_and_pending_callback_busy_keep_new_gate_ingress_atomic() {
    let mut no_legacy = EngineInstance::new(1, KEY).unwrap();
    no_legacy.map(CODE, 1, 7).unwrap();
    upload(&mut no_legacy, CODE, &[0x0f, 0x0b]);
    let gate = compile(&mut no_legacy, &[(CODE, 2)], &[(CODE, 7)]);
    stop_input(&mut no_legacy, CODE, 7, u32::MAX);
    reject(
        &mut no_legacy,
        &[gate],
        HostError::InvalidArtifact,
        |engine| engine.capture_call(KEY, 0, CallingConvention32::Cdecl, 0),
    );

    const ENTRY: u32 = LEGACY + 0x100;
    const RETURN: u32 = LEGACY + 0x200;
    let (mut engine, keep) = fixture();
    upload(&mut engine, OTHER, &[0x0f, 0x0b]);
    upload(&mut engine, LEGACY, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90, 0xc3]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    engine.map(0x8000, 1, 3).unwrap();
    engine.write32(0x8010, KEEP).unwrap();
    describe(
        &mut engine,
        &[(LEGACY, 2), (ENTRY, 2), (RETURN, 2)],
        &[(LEGACY, 17), (RETURN, 18)],
    );
    assert_eq!(engine.compile_with_gates(3, 2), Ok(2));
    let gate = compile(&mut engine, &[(CODE, 2)], &[(CODE, 7)]);
    stop_input(&mut engine, CODE, 7, u32::MAX);
    reject(
        &mut engine,
        &[keep, gate],
        HostError::Call(CallError::InvalidStop),
        |engine| engine.capture_call(KEY, 2, CallingConvention32::Cdecl, 0),
    );
    reject(
        &mut engine,
        &[keep, gate],
        HostError::InvalidArtifact,
        |engine| engine.capture_call(KEY ^ 1, 2, CallingConvention32::Cdecl, 0),
    );
    // typed legacy stop input parks only the existing owner for busy checks.
    stop_input(&mut engine, LEGACY, 17, 0x8010);
    let outer = engine
        .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
        .unwrap();
    for callback in [false, true] {
        let callback_token = if callback {
            Some(
                engine
                    .begin_callback(KEY, 2, outer.token, ENTRY, RETURN, 18, &[])
                    .unwrap()
                    .token,
            )
        } else {
            None
        };
        describe(&mut engine, &[(OTHER, 2)], &[(OTHER, 9)]);
        for (blocks, gates) in [(0, u32::MAX), (1, 1), (9, 0)] {
            reject(
                &mut engine,
                &[keep, gate],
                HostError::Call(CallError::Busy),
                |engine| engine.compile_resident_with_gates(blocks, gates),
            );
        }
        reject(
            &mut engine,
            &[keep, gate],
            HostError::Call(CallError::Busy),
            |engine| engine.guard_resident(KEY, gate),
        );
        reject(
            &mut engine,
            &[keep, gate],
            HostError::InvalidArtifact,
            |engine| engine.guard_resident(KEY ^ 1, gate),
        );
        reject(
            &mut engine,
            &[keep, gate],
            HostError::Resident(RegistryError::InvalidUnit),
            |engine| engine.guard_resident(KEY, 0),
        );
        let before = saved(&engine, &[keep, gate], &[CODE, KEEP]);
        assert_eq!(engine.lookup_resident(CODE).map(|id| id.get()), Ok(gate));
        assert_eq!(saved(&engine, &[keep, gate], &[CODE, KEEP]), before);
        if let Some(token) = callback_token {
            assert_eq!(engine.guard(KEY, 2), Ok(()));
            engine.abort_callback(KEY, token).unwrap();
        }
    }
    engine.abandon_call(KEY, outer.token).unwrap();
    assert_eq!(engine.guard(KEY, 2), Ok(()));
    assert_eq!(engine.guard_resident(KEY, gate), Ok(()));
    let new = compile(&mut engine, &[(OTHER, 2)], &[(OTHER, 9)]);
    assert!(new > gate);
    assert_eq!(engine.lookup_resident(OTHER).map(|id| id.get()), Ok(new));
}
