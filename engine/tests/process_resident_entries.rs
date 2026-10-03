use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{
        ExecutionExit, ExitReason, UnsupportedFeature,
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{State32, decode::DecodeError},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const A: u32 = 0x1000;
const B: u32 = 0x2000;
const C: u32 = 0x3000;
const STACK: u32 = 0x8000;
const OUTER: u32 = C + 0x100;
const CALLBACK: u32 = C + 0x200;
const RETURN: u32 = C + 0x300;

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn entries(engine: &mut EngineInstance, seeds: &[u32], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    for (index, seed) in seeds.iter().enumerate() {
        transfer[index * 4..index * 4 + 4].copy_from_slice(&seed.to_le_bytes());
    }
    for (index, (pc, id)) in gates.iter().enumerate() {
        let offset = seeds.len() * 4 + index * 8;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
    }
}

fn blocks(engine: &mut EngineInstance, specs: &[(u32, u32)], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0x5a);
    for (index, (pc, value)) in specs.iter().chain(gates).enumerate() {
        let offset = index * 8;
        transfer[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        transfer[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    for pc in [A, B, C, STACK] {
        engine.map(pc, 1, 7).unwrap();
    }
    for pc in [A, B, C] {
        upload(&mut engine, pc, &[0x40, 0xeb, 0, 0x0f, 0x0b]);
    }
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    upload(&mut engine, CALLBACK, &[0x90, 0xc3, 0x0f, 0x0b]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    upload(&mut engine, STACK + 0x10, &B.to_le_bytes());
    engine
}

fn compile(engine: &mut EngineInstance, pc: u32) -> u64 {
    entries(engine, &[pc], &[]);
    let before = engine.arena().to_vec();
    let id = engine.compile_resident_entries(1, 0).unwrap().get();
    assert_ne!(id, 0);
    assert_eq!(engine.arena(), before);
    id
}

#[derive(Debug, PartialEq, Eq)]
struct UnitSnapshot {
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    artifact: Result<Vec<u8>, HostError>,
    artifact_pointer: Option<usize>,
    dispatcher: Result<Vec<u8>, HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    units: Vec<UnitSnapshot>,
    installations: Vec<Result<ResidentInstallation, HostError>>,
}

fn snapshot(engine: &EngineInstance, ids: &[u64]) -> Snapshot {
    Snapshot {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        artifact: engine.artifact_bytes().map(<[u8]>::to_vec),
        artifact_pointer: engine
            .artifact_bytes()
            .ok()
            .map(|bytes| bytes.as_ptr() as usize),
        dispatcher: engine.dispatcher_bytes(KEY).map(<[u8]>::to_vec),
        ram: [A, B, C, STACK]
            .map(|pc| {
                let mut bytes = vec![0; 4096];
                engine
                    .memory()
                    .and_then(|memory| {
                        memory
                            .read(GuestAddress(pc), &mut bytes)
                            .map_err(HostError::Memory)
                    })
                    .map(|()| bytes)
            })
            .to_vec(),
        units: ids
            .iter()
            .map(|&id| UnitSnapshot {
                bytes: engine.resident_bytes(id).map(<[u8]>::to_vec),
                pointer: engine
                    .resident_bytes(id)
                    .ok()
                    .map(|bytes| bytes.as_ptr() as usize),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
        installations: [A, B, OUTER, CALLBACK, RETURN]
            .map(|pc| engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
    }
}

fn failed<T>(
    engine: &mut EngineInstance,
    ids: &[u64],
    error: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = snapshot(engine, ids);
    assert_eq!(operation(engine).err(), Some(error));
    assert_eq!(snapshot(engine, ids), before);
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

fn stop(engine: &mut EngineInstance, pc: u32, reason: ExitReason) {
    // native stop injection proves process admission; emitted wasm owns the actual execution proof.
    let mut state = State32 {
        eip: pc,
        ..State32::default()
    };
    state.registers[4] = STACK + 0x10;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 3, reason },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

#[test]
fn entry_pc_compiles_one_current_resident_unit_without_changing_arena() {
    let mut engine = EngineInstance::new(1, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[0x40, 0xeb, 0]);
    engine.upload(0x1000, 3).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    let arena = engine.arena().to_vec();
    let id = engine.compile_resident_entries(1, 0).unwrap().get();
    assert_ne!(id, 0);
    assert_eq!(engine.lookup_resident(0x1000).unwrap().get(), id);
    assert_eq!(engine.lookup_resident(0x1001).unwrap().get(), id);
    assert!(engine.resident_bytes(id).unwrap().starts_with(b"\0asm"));
    assert_eq!(engine.guard_resident(7, id), Ok(()));
    assert_eq!(engine.generation(), 0);
    assert_eq!(engine.arena(), arena);
}

#[test]
fn eighth_le4_seed_and_gate_pairs_follow_entry_count_not_block_stride() {
    let mut engine = fixture();
    let seeds = [
        A + 0x90,
        A + 0x30,
        A + 0x70,
        A + 0x10,
        A + 0x50,
        A + 0xb0,
        A + 0xd0,
        A + 0xf0,
    ];
    for pc in seeds {
        upload(&mut engine, pc, &[0x90, 0xeb, 0, 0x0f, 0x0b]);
    }
    for pc in [seeds[1], seeds[7]] {
        upload(&mut engine, pc, &[0x0f, 0x0b]);
    }
    entries(&mut engine, &seeds, &[(seeds[7], 29), (seeds[1], 17)]);
    let before = snapshot(&engine, &[]);
    let id = engine.compile_resident_entries(8, 2).unwrap().get();
    assert_ne!(id, 0);
    assert_eq!(snapshot(&engine, &[]), before);
    for pc in seeds {
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
    }
    assert_eq!(engine.lookup_resident(seeds[6] + 1).unwrap().get(), id);
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
    assert_eq!(
        engine.guard_resident(KEY ^ 1, id),
        Err(HostError::InvalidArtifact)
    );
}

#[test]
fn current_resident_instruction_families_compile_without_guest_data_or_stack_reads() {
    // the unmapped data address must not be read during cold preparation.
    let cases: &[(&[u8], bool)] = &[
        (&[0xb8, 0x78, 0x56, 0x34, 0x12], false),
        (&[0x8d, 0x40, 7], false),
        (&[0x0f, 0xb6, 0xc1], false),
        (&[0x40, 0x83, 0xe8, 1, 0x85, 0xc0], false),
        (&[0xa1, 0, 0, 0xad, 0xde], false),
        (&[0x0f, 0xb6, 0x05, 0, 0, 0xad, 0xde], false),
        (&[0x03, 0x05, 0, 0, 0xad, 0xde], false),
        (&[0x85, 0x05, 0, 0, 0xad, 0xde], false),
        (&[0xa3, 0, 0, 0xad, 0xde], false),
        (&[0xff, 0x05, 0, 0, 0xad, 0xde], false),
        (&[0x83, 0x05, 0, 0, 0xad, 0xde, 1], false),
        (&[0x50, 0x59], false),
        (&[0xff, 0x35, 0, 0, 0xad, 0xde], false),
        (&[0x8f, 0x05, 0, 0, 0xad, 0xde], false),
        (&[0xe9, 0, 0, 0, 0], true),
        (&[0x75, 0], true),
        (&[0xff, 0xe0], true),
        (&[0xff, 0x25, 0, 0, 0xad, 0xde], true),
        (&[0xe8, 0, 0, 0, 0], true),
        (&[0xff, 0xd0], true),
        (&[0xff, 0x15, 0, 0, 0xad, 0xde], true),
        (&[0xc3], true),
        (&[0xc2, 8, 0], true),
    ];
    for &(prefix, terminal) in cases {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(A, 1, 7).unwrap();
        let mut code = prefix.to_vec();
        if !terminal {
            code.extend([0xeb, 0]);
        }
        code.extend([0x0f, 0x0b]);
        upload(&mut engine, A, &code);
        entries(&mut engine, &[A], &[]);
        let before = snapshot(&engine, &[]);
        let id = engine.compile_resident_entries(1, 0).unwrap().get();
        assert_eq!(
            engine.lookup_resident(A).unwrap().get(),
            id,
            "{prefix:02x?}"
        );
        assert_eq!(snapshot(&engine, &[]), before, "{prefix:02x?}");
    }
}

#[test]
fn seed_boundaries_stop_without_a_branch_and_reject_crossing_instruction() {
    let mut engine = fixture();
    upload(&mut engine, A, &[0x90, 0x40, 0xeb, 0, 0x0f, 0x0b]);
    entries(&mut engine, &[A + 1, A], &[]);
    let id = engine.compile_resident_entries(2, 0).unwrap().get();
    for pc in [A, A + 1, A + 2] {
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
    }
    upload(&mut engine, C, &[0xb8, 0x90, 0x90, 0x90, 0x90, 0xeb, 0]);
    entries(&mut engine, &[C, C + 2], &[]);
    failed(
        &mut engine,
        &[id],
        compile_error(CompileError::InvalidBlocks),
        |engine| engine.compile_resident_entries(2, 0),
    );
}

#[test]
fn terminal_at_last_guest_bytes_omits_successor_page_and_preserves_snapshot_scope() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(A, 1, 7).unwrap();
    upload(&mut engine, A + 0xffe, &[0xeb, 0]);
    let id = compile(&mut engine, A + 0xffe);
    let bytes = engine.resident_bytes(id).unwrap().to_vec();
    let pointer = engine.resident_bytes(id).unwrap().as_ptr();
    engine.map(B, 1, 7).unwrap();
    upload(&mut engine, B, &[0x0f, 0x06]);
    assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
    assert_eq!(engine.resident_bytes(id).unwrap().as_ptr(), pointer);
    upload(&mut engine, A + 0xffe, &[0xeb, 0]);
    assert_eq!(
        engine.resident_bytes(id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );

    let mut last = EngineInstance::new(1, KEY).unwrap();
    last.map(0xffff_f000, 1, 7).unwrap();
    upload(&mut last, u32::MAX, &[0xc3]);
    let id = compile(&mut last, u32::MAX);
    assert_eq!(last.lookup_resident(u32::MAX).unwrap().get(), id);
}

#[test]
fn unterminated_or_cross_page_instructions_report_execute_fault_before_publication() {
    for (address, bytes, fault_address, length) in [
        (A + 0xfff, &[0x90][..], B, 1),
        (A + 0xffe, &[0xb8, 0x12][..], B, 3),
    ] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.map(A, 1, 7).unwrap();
        upload(&mut engine, address, bytes);
        entries(&mut engine, &[address], &[]);
        let pc = if bytes == [0x90] { B } else { address };
        failed(
            &mut engine,
            &[],
            instruction_error(
                pc,
                InstructionError::Decode(DecodeError::MemoryFault {
                    pc: GuestAddress(pc),
                    fault: MemoryFault {
                        address: GuestAddress(fault_address),
                        access: Access::Execute,
                        reason: FaultReason::Unmapped,
                    },
                    length,
                }),
            ),
            |engine| engine.compile_resident_entries(1, 0),
        );
    }
    let mut engine = fixture();
    engine.protect(C, 1, 1).unwrap();
    entries(&mut engine, &[C], &[]);
    failed(
        &mut engine,
        &[],
        instruction_error(
            C,
            InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(C),
                fault: MemoryFault {
                    address: GuestAddress(C),
                    access: Access::Execute,
                    reason: FaultReason::Permission,
                },
                length: 1,
            }),
        ),
        |engine| engine.compile_resident_entries(1, 0),
    );
    engine.protect(C, 1, 5).unwrap();
    assert!(engine.compile_resident_entries(1, 0).is_ok());
}

#[test]
fn exact_sixty_four_instruction_limit_counts_gates_and_ignores_terminal_poison() {
    let mut engine = fixture();
    let mut valid = vec![0x90; 63];
    valid.extend([0xeb, 0, 0x0f, 0x06]);
    upload(&mut engine, A, &valid);
    let id = compile(&mut engine, A);
    assert_eq!(engine.lookup_resident(A + 63).unwrap().get(), id);
    let mut too_many = vec![0x90; 64];
    too_many.extend([0xeb, 0]);
    upload(&mut engine, B, &too_many);
    entries(&mut engine, &[B], &[]);
    failed(
        &mut engine,
        &[id],
        compile_error(CompileError::InstructionLimit),
        |engine| engine.compile_resident_entries(1, 0),
    );
    entries(&mut engine, &[A, OUTER], &[(OUTER, 17)]);
    failed(
        &mut engine,
        &[id],
        compile_error(CompileError::InstructionLimit),
        |engine| engine.compile_resident_entries(2, 1),
    );
    let mut with_gate = vec![0x90; 62];
    with_gate.extend([0xeb, 0]);
    upload(&mut engine, C, &with_gate);
    entries(&mut engine, &[C, OUTER], &[(OUTER, 17)]);
    let new_id = engine.compile_resident_entries(2, 1).unwrap().get();
    assert_eq!(engine.lookup_resident(OUTER).unwrap().get(), new_id);
}

#[test]
fn invalid_counts_seeds_gates_and_unsupported_profile_are_failure_atomic() {
    let mut engine = fixture();
    let id = compile(&mut engine, B);
    for (count, gate_count) in [(0, 0), (9, 0), (u32::MAX, 0), (1, 2), (1, u32::MAX)] {
        failed(&mut engine, &[id], HostError::InvalidRequest, |engine| {
            engine.compile_resident_entries(count, gate_count)
        });
    }
    entries(&mut engine, &[A, A], &[]);
    failed(
        &mut engine,
        &[id],
        compile_error(CompileError::InvalidBlocks),
        |engine| engine.compile_resident_entries(2, 0),
    );
    for gates in [
        vec![(OUTER, 0)],
        vec![(C + 0x400, 17)],
        vec![(OUTER, 17), (RETURN, 17)],
        vec![(OUTER, 17), (OUTER, 18)],
    ] {
        entries(&mut engine, &[OUTER, RETURN], &gates);
        failed(
            &mut engine,
            &[id],
            compile_error(CompileError::InvalidGates),
            |engine| engine.compile_resident_entries(2, gates.len() as u32),
        );
    }
    entries(&mut engine, &[A], &[(A, 17)]);
    failed(
        &mut engine,
        &[id],
        instruction_error(A, InstructionError::InvalidGate),
        |engine| engine.compile_resident_entries(1, 1),
    );
    for (code, feature) in [
        (&[0x0f, 0x06][..], UnsupportedFeature::Privileged),
        (&[0x0f, 0x0b][..], UnsupportedFeature::Opcode),
    ] {
        upload(&mut engine, A, code);
        entries(&mut engine, &[A], &[]);
        failed(
            &mut engine,
            &[id],
            instruction_error(
                A,
                InstructionError::Decode(DecodeError::Unsupported(feature)),
            ),
            |engine| engine.compile_resident_entries(1, 0),
        );
    }
    upload(&mut engine, A, &[0x90, 0xeb, 0]);
    entries(&mut engine, &[A], &[]);
    assert!(engine.compile_resident_entries(1, 0).is_ok());
}

#[test]
fn successors_keep_full_u64_old_owners_and_allow_only_stale_instruction_collisions() {
    let mut engine = fixture();
    blocks(&mut engine, &[(B, 3)], &[]);
    assert_eq!(engine.compile(1), Ok(1));
    blocks(&mut engine, &[(B, 3)], &[]);
    let old_explicit = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, old_explicit, 0)
        .unwrap();
    let original = compile(&mut engine, A);
    engine
        .acknowledge_resident_installation(KEY, original, 1)
        .unwrap();
    entries(&mut engine, &[A], &[]);
    failed(
        &mut engine,
        &[old_explicit, original],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(A),
        }),
        |engine| engine.compile_resident_entries(1, 0),
    );
    upload(&mut engine, A, &[0x40, 0xeb, 0]);
    assert_eq!(
        engine.guard_resident(KEY, original),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(B), 3)
        .unwrap();
    entries(&mut engine, &[A], &[]);
    let before = snapshot_state_without_new_unit(&engine, &[old_explicit, original]);
    let successor = engine.compile_resident_entries(1, 0).unwrap().get();
    assert_ne!(successor, original);
    assert_ne!(successor, old_explicit);
    assert_eq!(
        snapshot_state_without_new_unit(&engine, &[old_explicit, original]),
        before
    );
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(engine.lookup_resident(A).unwrap().get(), successor);
    assert_eq!(engine.guard_resident(KEY, successor), Ok(()));
    assert_eq!(engine.guard_resident(KEY, old_explicit), Ok(()));
    assert_eq!(
        engine.guard_resident(KEY ^ 1, successor),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.guard_resident(KEY, 0),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    assert_eq!(engine.lookup_installed_resident(KEY, B).unwrap().slot, 0);
    assert_eq!(
        engine.lookup_installed_resident(KEY, A),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(A)
        }))
    );
    failed(
        &mut engine,
        &[old_explicit, original, successor],
        HostError::InvalidRequest,
        |engine| engine.acknowledge_resident_installation(KEY, successor, 1),
    );
    engine
        .acknowledge_resident_installation(KEY, successor, 2)
        .unwrap();
    assert_eq!(engine.lookup_installed_resident(KEY, A).unwrap().slot, 2);
}

fn snapshot_state_without_new_unit(engine: &EngineInstance, ids: &[u64]) -> Snapshot {
    // current-pc lookup intentionally changes to the newly admitted successor.
    let mut snapshot = snapshot(engine, ids);
    snapshot.installations.clear();
    snapshot
}

#[test]
fn distinct_overlapping_decodings_keep_existing_instruction_start_admission_policy() {
    let mut engine = fixture();
    upload(&mut engine, A, &[0xb8, 0x90, 0x90, 0xeb, 0, 0xeb, 0]);
    let original = compile(&mut engine, A);
    let overlapping = compile(&mut engine, A + 1);
    assert_ne!(original, overlapping);
    for (pc, id) in [
        (A, original),
        (A + 5, original),
        (A + 1, overlapping),
        (A + 2, overlapping),
        (A + 3, overlapping),
    ] {
        assert_eq!(engine.lookup_resident(pc).unwrap().get(), id);
    }
    entries(&mut engine, &[A + 3], &[]);
    failed(
        &mut engine,
        &[original, overlapping],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(A + 3),
        }),
        |engine| engine.compile_resident_entries(1, 0),
    );
}

#[test]
fn eighth_unit_remains_available_and_capacity_precedes_decode_but_not_invalid_counts() {
    let mut engine = fixture();
    for index in 0..8 {
        upload(&mut engine, A + index * 16, &[0x40, 0xeb, 0]);
    }
    let ids: Vec<_> = (0..8)
        .map(|index| compile(&mut engine, A + index * 16))
        .collect();
    assert_eq!(
        ids.iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8
    );
    upload(&mut engine, C, &[0x0f, 0x06]);
    entries(&mut engine, &[C], &[]);
    failed(
        &mut engine,
        &ids,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident_entries(1, 0),
    );
    entries(&mut engine, &[u32::MAX, u32::MAX], &[]);
    failed(
        &mut engine,
        &ids,
        HostError::Resident(RegistryError::UnitCapacity),
        |engine| engine.compile_resident_entries(2, 0),
    );
    failed(&mut engine, &ids, HostError::InvalidRequest, |engine| {
        engine.compile_resident_entries(9, 0)
    });
    for (index, &id) in ids.iter().enumerate() {
        assert_eq!(
            engine.lookup_resident(A + index as u32 * 16).unwrap().get(),
            id
        );
    }
}

#[test]
fn ordinary_compile_ignores_cancel_while_installation_keeps_its_existing_cancel_policy() {
    let mut engine = fixture();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    let id = compile(&mut engine, A);
    failed(
        &mut engine,
        &[id],
        HostError::Call(CallError::Cancelled),
        |engine| engine.acknowledge_resident_installation(KEY, id, 0),
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    assert_eq!(
        engine
            .acknowledge_resident_installation(KEY, id, 0)
            .unwrap()
            .slot,
        0
    );
}

#[test]
fn pending_call_and_replacement_callback_busy_precede_descriptor_and_cancel_controls() {
    let mut engine = fixture();
    blocks(
        &mut engine,
        &[(OUTER, 2), (CALLBACK, 2), (RETURN, 2)],
        &[(OUTER, 17), (RETURN, 18)],
    );
    assert_eq!(engine.compile_with_gates(3, 2), Ok(1));
    let a = compile(&mut engine, A);
    stop(&mut engine, OUTER, ExitReason::Gate { id: 17 });
    let outer = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    entries(&mut engine, &[u32::MAX], &[]);
    for (count, gate_count) in [(0, 0), (u32::MAX, u32::MAX), (1, 0)] {
        failed(
            &mut engine,
            &[a],
            HostError::Call(CallError::Busy),
            |engine| engine.compile_resident_entries(count, gate_count),
        );
    }
    let callback = engine
        .begin_callback(KEY, 1, outer.token, CALLBACK, RETURN, 18, &[])
        .unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    failed(
        &mut engine,
        &[a],
        HostError::Call(CallError::Busy),
        |engine| engine.compile_resident_entries(0, u32::MAX),
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.abort_callback(KEY, callback.token).unwrap();
    engine
        .complete_call(KEY, 1, outer.token, 0x1234_5678)
        .unwrap();
    let b = compile(&mut engine, B);
    assert_eq!(engine.guard_resident(KEY, b), Ok(()));
}

#[test]
fn unarmed_and_armed_resident_callback_both_reject_ordinary_entry_compilation() {
    let mut engine = fixture();
    entries(&mut engine, &[OUTER], &[(OUTER, 17)]);
    let outer_id = engine.compile_resident_entries(1, 1).unwrap().get();
    entries(&mut engine, &[CALLBACK, RETURN], &[(RETURN, 18)]);
    let callback_id = engine.compile_resident_entries(2, 1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, outer_id, 0)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, callback_id, 1)
        .unwrap();
    stop(&mut engine, OUTER, ExitReason::Gate { id: 17 });
    let outer = engine
        .capture_resident_call(KEY, outer_id, CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = engine
        .begin_resident_callback(
            KEY,
            outer_id,
            callback_id,
            outer.token,
            CALLBACK,
            RETURN,
            18,
            &[],
        )
        .unwrap();
    entries(&mut engine, &[A], &[]);
    failed(
        &mut engine,
        &[outer_id, callback_id],
        HostError::Call(CallError::Busy),
        |engine| engine.compile_resident_entries(1, 0),
    );
    engine
        .authorize_resident_callback(KEY, callback_id, callback.token)
        .unwrap();
    failed(
        &mut engine,
        &[outer_id, callback_id],
        HostError::Call(CallError::Busy),
        |engine| engine.compile_resident_entries(0, u32::MAX),
    );
    engine.abort_callback(KEY, callback.token).unwrap();
    engine
        .complete_resident_call(KEY, outer_id, outer.token, 9)
        .unwrap();
    assert!(engine.compile_resident_entries(1, 0).is_ok());
}

#[test]
fn closed_precedes_invalid_counts_and_preserves_tombstone_arena() {
    let mut engine = fixture();
    let id = compile(&mut engine, A);
    entries(&mut engine, &[B], &[]);
    engine.close();
    for (count, gates) in [(1, 0), (0, 0), (u32::MAX, u32::MAX)] {
        failed(&mut engine, &[id], HostError::Closed, |engine| {
            engine.compile_resident_entries(count, gates)
        });
    }
}
