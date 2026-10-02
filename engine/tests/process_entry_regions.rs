use ring3_engine::{
    abi::x86::{decode_exit, decode_state, encode_exit_v3, encode_state},
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{CompileError, InstructionError},
        x86::{State32, decode::DecodeError},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0xfedc_ba98_1234_5678;
const OUTER: u32 = 0x1000;
const ENTRY: u32 = 0x1100;
const RETURN: u32 = 0x1200;
const INNER: u32 = 0x1300;
const TARGET: u32 = 0x4000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn word(engine: &mut EngineInstance, address: u32, value: u32) {
    upload(engine, address, &value.to_le_bytes());
}

fn entries(engine: &mut EngineInstance, seeds: &[u32], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xcc);
    for (index, entry) in seeds.iter().enumerate() {
        transfer[index * 4..index * 4 + 4].copy_from_slice(&entry.to_le_bytes());
    }
    for (index, (entry, id)) in gates.iter().enumerate() {
        let offset = seeds.len() * 4 + index * 8;
        transfer[offset..offset + 4].copy_from_slice(&entry.to_le_bytes());
        transfer[offset + 4..offset + 8].copy_from_slice(&id.to_le_bytes());
    }
}

fn guest_bytes(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

fn unchanged<T>(
    engine: &mut EngineInstance,
    error: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let arena = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().map(|bytes| bytes.to_vec());
    let generation = engine.generation();
    assert_eq!(operation(engine).err(), Some(error));
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        artifact
    );
    assert_eq!(engine.generation(), generation);
}

fn stop(engine: &mut EngineInstance, state: State32, reason: ExitReason, retired: u32) {
    // native stop injection proves process admission; actual CPU execution has a separate oracle.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired, reason },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

fn original() -> State32 {
    State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            0x8010,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: OUTER,
        eflags: 0xcd7,
    }
}

fn active() -> EngineInstance {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for (address, bits) in [(0x1000, 7), (TARGET, 7), (0x8000, 7)] {
        engine.map(address, 1, bits).unwrap();
    }
    for address in [OUTER, RETURN, INNER] {
        upload(&mut engine, address, &[0x0f, 0x0b]);
    }
    upload(&mut engine, ENTRY, &[0x90, 0xc2, 8, 0]);
    upload(&mut engine, TARGET, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
    for (address, value) in [
        (0x8010, 0x2000),
        (0x8014, 0xfedc_ba98),
        (0x8018, 0x7654_3210),
    ] {
        word(&mut engine, address, value);
    }
    for (index, value) in [
        OUTER, 2, ENTRY, 4, RETURN, 2, INNER, 2, OUTER, 17, RETURN, 18, INNER, 19,
    ]
    .into_iter()
    .enumerate()
    {
        engine.arena_mut().unwrap()[140 + index * 4..144 + index * 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(engine.compile_with_gates(4, 3), Ok(1));
    stop(&mut engine, original(), ExitReason::Gate { id: 17 }, 3);
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Stdcall, 2)
            .unwrap()
            .token,
        1
    );
    assert_eq!(
        engine
            .begin_callback(KEY, 1, 1, ENTRY, RETURN, 18, &[0x11, 0x22])
            .unwrap()
            .token,
        2
    );
    let mut stopped = decode_state(&engine.arena()[..56]).unwrap();
    stopped.eip = TARGET;
    stop(&mut engine, stopped, ExitReason::NeedCode, 7);
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    engine
}

fn replacement(engine: &mut EngineInstance) {
    entries(
        engine,
        &[OUTER, TARGET, RETURN, INNER],
        &[(OUTER, 17), (RETURN, 18), (INNER, 19)],
    );
}

fn callback_wire(generation: u32) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [
        2_u32, 1, 1, 0, ENTRY, 0x8004, RETURN, 18, 2, 0, generation, 0,
    ]
    .into_iter()
    .enumerate()
    {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn ordinary_ingress_reads_the_eighth_le4_seed_and_final_gate_pair() {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for (address, bits) in [(0x1000, 7), (0x2000, 7), (0x8000, 3)] {
        engine.map(address, 1, bits).unwrap();
    }
    let gates: Vec<_> = (0..7)
        .map(|index| (0x1000 + index * 16, 0x8000_0001 + index))
        .collect();
    for &(address, _) in &gates {
        upload(&mut engine, address, &[0x0f, 0x0b]);
    }
    upload(&mut engine, 0x2000, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
    word(&mut engine, 0x8000, 0x3000);
    let mut seeds: Vec<_> = gates.iter().map(|&(address, _)| address).collect();
    seeds.push(0x2000);
    entries(&mut engine, &seeds, &gates);
    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile_entries(8, 7), Ok(1));
    assert_eq!(engine.arena(), arena);
    seeds[7] = 0x3000;
    entries(&mut engine, &seeds, &gates);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(0x3000),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(0x3000),
                fault: MemoryFault {
                    address: GuestAddress(0x3000),
                    access: Access::Execute,
                    reason: FaultReason::Unmapped,
                },
                length: 1,
            }),
        }),
        |engine| engine.compile_entries(8, 7),
    );
    seeds[7] = 0x2000;
    let mut bad_gates = gates.clone();
    bad_gates[6].1 = 0;
    entries(&mut engine, &seeds, &bad_gates);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::InvalidGates),
        |engine| engine.compile_entries(8, 7),
    );
    entries(&mut engine, &seeds, &gates);
    assert_eq!(engine.compile_entries(8, 7), Ok(2));
    let mut state = State32 {
        eip: gates[6].0,
        ..State32::default()
    };
    state.registers[4] = 0x8000;
    stop(&mut engine, state, ExitReason::Gate { id: gates[6].1 }, 0);
    let record = engine
        .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(
        (record.token, record.id, record.return_pc),
        (1, 0x8000_0007, 0x3000)
    );
}

#[test]
fn all_eight_gate_seeds_and_the_final_id_are_admitted() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    let gates: Vec<_> = (0..8)
        .map(|index| (0x1000 + index * 16, 0x1020_3001 + index))
        .collect();
    for &(address, _) in &gates {
        upload(&mut engine, address, &[0x0f, 0x0b]);
    }
    word(&mut engine, 0x8000, 0x2000);
    let seeds: Vec<_> = gates.iter().map(|&(address, _)| address).collect();
    entries(&mut engine, &seeds, &gates);
    assert_eq!(engine.compile_entries(8, 8), Ok(1));
    let mut state = State32 {
        eip: gates[7].0,
        ..State32::default()
    };
    state.registers[4] = 0x8000;
    stop(&mut engine, state, ExitReason::Gate { id: gates[7].1 }, 0);
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
            .unwrap()
            .id,
        0x1020_3008
    );
}

#[test]
fn ordinary_compile_keeps_open_busy_count_order_and_ignores_cpu_record_contents() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    upload(&mut engine, 0x1000, &[0x90, 0xeb, 0x7f]);
    engine.arena_mut().unwrap()[..100].fill(0xff);
    for (count, gates) in [(0, 0), (9, 0), (1, 2), (u32::MAX, u32::MAX)] {
        unchanged(&mut engine, HostError::InvalidRequest, |engine| {
            engine.compile_entries(count, gates)
        });
    }
    entries(&mut engine, &[0x1000], &[]);
    let arena = engine.arena().to_vec();
    assert_eq!(engine.compile_entries(1, 0), Ok(1));
    assert_eq!(engine.arena(), arena);
    let mut active = active();
    unchanged(&mut active, HostError::Call(CallError::Busy), |engine| {
        engine.compile_entries(0, u32::MAX)
    });
    active.abort_callback(KEY, 2).unwrap();
    unchanged(&mut active, HostError::Call(CallError::Busy), |engine| {
        engine.compile_entries(0, u32::MAX)
    });
    active.close();
    unchanged(&mut active, HostError::Closed, |engine| {
        engine.compile_entries(0, u32::MAX)
    });
}

#[test]
fn failed_discovery_preserves_artifact_arena_and_stamps_then_retry_spends_one_generation() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for address in [0x1000, 0x4000] {
        engine.map(address, 1, 7).unwrap();
    }
    upload(&mut engine, 0x1000, &[0x90, 0xeb, 0x7f]);
    upload(&mut engine, 0x4000, &[0x40]);
    entries(&mut engine, &[0x1000], &[]);
    assert_eq!(engine.compile_entries(1, 0), Ok(1));
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 3)
        .unwrap();
    let unsupported = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x4000), 1)
        .unwrap();
    entries(&mut engine, &[0x4000], &[]);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(0x4000),
            cause: InstructionError::BackendUnsupported,
        }),
        |engine| engine.compile_entries(1, 0),
    );
    assert!(engine.memory().unwrap().is_code_current(&code));
    assert!(engine.memory().unwrap().is_code_current(&unsupported));
    assert_eq!(guest_bytes(&engine, 0x4000, 1), [0x40]);
    upload(&mut engine, 0x4000, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
    entries(&mut engine, &[0x4000], &[]);
    assert_eq!(engine.compile_entries(1, 0), Ok(2));
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}

#[test]
fn callback_entry_install_preserves_records_and_ram_then_migrated_finish_or_abort_restores_outer() {
    for finish in [true, false] {
        let mut engine = active();
        replacement(&mut engine);
        let arena = engine.arena().to_vec();
        let stack = guest_bytes(&engine, 0x8000, 32);
        let snapshots = [OUTER, TARGET, 0x8000].map(|pc| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(pc), 1)
                .unwrap()
        });
        assert_eq!(engine.resume_callback_entries(KEY, 1, 2, 4, 3), Ok(2));
        let mut expected = arena;
        expected[140..204].copy_from_slice(&callback_wire(2));
        assert_eq!(engine.arena(), expected);
        assert_eq!(guest_bytes(&engine, 0x8000, 32), stack);
        for snapshot in snapshots {
            assert!(engine.memory().unwrap().is_code_current(&snapshot));
        }
        assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
        engine.guard(KEY, 2).unwrap();
        if finish {
            let mut returned = decode_state(&engine.arena()[..56]).unwrap();
            returned.eip = RETURN;
            returned.registers[4] = 0x8010;
            returned.registers[0] = 0x8765_4321;
            stop(&mut engine, returned, ExitReason::Gate { id: 18 }, 2);
            assert_eq!(
                engine.finish_callback(KEY, 2, 2).unwrap().result,
                0x8765_4321
            );
        } else {
            engine.abort_callback(KEY, 2).unwrap();
        }
        assert_eq!(decode_state(&engine.arena()[..56]), Ok(original()));
        assert_eq!(
            decode_exit(&engine.arena()[56..96]).unwrap(),
            ExecutionExit {
                retired: 3,
                reason: ExitReason::Gate { id: 17 }
            }
        );
        unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
            engine.complete_call(KEY, 1, 1, 99)
        });
        engine.complete_call(KEY, 2, 1, 99).unwrap();
        let mut completed = original();
        completed.registers[0] = 99;
        completed.registers[4] = 0x801c;
        completed.eip = 0x2000;
        assert_eq!(decode_state(&engine.arena()[..56]), Ok(completed));
        stop(&mut engine, original(), ExitReason::Gate { id: 17 }, 0);
        assert_eq!(
            engine
                .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
                .unwrap()
                .token,
            3
        );
    }
}

#[test]
fn callback_entry_discovery_errors_and_count_priority_preserve_authority_before_corrected_retry() {
    let mut engine = active();
    replacement(&mut engine);
    let records = engine.arena()[..96].to_vec();
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[96] = 1;
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.resume_callback_entries(KEY ^ (1 << 32), 1, 0, 0, 0)
    });
    unchanged(
        &mut engine,
        HostError::Call(CallError::InvalidToken),
        |engine| engine.resume_callback_entries(KEY, 1, 0, 0, 0),
    );
    unchanged(&mut engine, HostError::InvalidRequest, |engine| {
        engine.resume_callback_entries(KEY, 1, 2, 0, 0)
    });
    unchanged(
        &mut engine,
        HostError::Call(CallError::InvalidStop),
        |engine| engine.resume_callback_entries(KEY, 1, 2, 4, 3),
    );
    engine.arena_mut().unwrap()[..96].copy_from_slice(&records);
    unchanged(
        &mut engine,
        HostError::Call(CallError::Cancelled),
        |engine| engine.resume_callback_entries(KEY, 1, 2, 4, 3),
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    entries(&mut engine, &[TARGET, TARGET], &[]);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::InvalidBlocks),
        |engine| engine.resume_callback_entries(KEY, 1, 2, 2, 0),
    );
    entries(
        &mut engine,
        &[OUTER, TARGET, RETURN],
        &[(OUTER, 17), (RETURN, 19)],
    );
    unchanged(&mut engine, HostError::InvalidRequest, |engine| {
        engine.resume_callback_entries(KEY, 1, 2, 3, 2)
    });
    replacement(&mut engine);
    assert_eq!(engine.resume_callback_entries(KEY, 1, 2, 4, 3), Ok(2));
    assert_eq!(&engine.arena()[140..204], &callback_wire(2));
    engine.abort_callback(KEY, 2).unwrap();
    engine.complete_call(KEY, 2, 1, 7).unwrap();
}
