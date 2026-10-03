use ring3_engine::{
    abi::{
        arena::{CANCEL_OFFSET, EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const OUTER: u32 = 0x4000;
const HOME: u32 = 0x5000;
const RETURN: u32 = 0x5100;
const HOME_GATE: u32 = 0x5200;
const COLD: u32 = 0x6000;
const PAGES: [u32; 6] = [0x3000, OUTER, HOME, COLD, 0x7000, 0x8000];

fn descriptors(engine: &mut EngineInstance, pairs: &[(u32, u32)]) {
    for (index, &(pc, value)) in pairs.iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 8;
        let arena = engine.arena_mut().unwrap();
        arena[offset..offset + 4].copy_from_slice(&pc.to_le_bytes());
        arena[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn stop(engine: &mut EngineInstance, state: State32, reason: ExitReason) {
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 2, reason },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    home: u64,
    token: u32,
    frozen_outer: Vec<u8>,
    cold_state: State32,
}

fn fixture(authorized: bool) -> Fixture {
    let mut engine = EngineInstance::new(PAGES.len() as u32, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (OUTER, &[0x0f, 0x0b][..]),
        (HOME, &[0x90][..]),
        (RETURN, &[0x0f, 0x0b][..]),
        (HOME_GATE, &[0x0f, 0x0b][..]),
        (COLD, &[0x90, 0xeb, 0][..]),
        (0x6010, &[0x90][..]),
        (0x6020, &[0x90][..]),
        (0x6200, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    for pc in (1..=5).map(|index| 0x6300 + index * 0x10).chain([0x6400]) {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
        engine.upload(pc, 1).unwrap();
    }
    engine.write32(0x8ffc, 0x3005).unwrap();
    descriptors(&mut engine, &[(OUTER, 2), (OUTER, 17)]);
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    descriptors(
        &mut engine,
        &[
            (HOME, 1),
            (RETURN, 2),
            (HOME_GATE, 2),
            (RETURN, 18),
            (HOME_GATE, 19),
        ],
    );
    let home = engine.compile_resident_with_gates(3, 2).unwrap().get();
    engine.compile_with_gates(3, 2).unwrap();
    engine
        .acknowledge_resident_installation(KEY, outer, 0)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, home, 1)
        .unwrap();
    let outer_state = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: OUTER,
        eflags: 0xcd7,
    };
    // native stops are authored controls; the actual wasm fixture executes guest code.
    stop(&mut engine, outer_state, ExitReason::Gate { id: 17 });
    let frozen_outer = engine.arena()[..EXIT_OFFSET + EXIT_SIZE].to_vec();
    let outer_call = engine
        .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = engine
        .begin_resident_callback(KEY, outer, home, outer_call.token, HOME, RETURN, 18, &[])
        .unwrap();
    if authorized {
        engine
            .authorize_resident_callback(KEY, home, callback.token)
            .unwrap();
    }
    let mut cold_state = outer_state;
    cold_state.eip = COLD;
    cold_state.registers[0] = 13;
    cold_state.registers[4] = 0x8ff8;
    stop(&mut engine, cold_state, ExitReason::NeedCode);
    descriptors(&mut engine, &[(COLD, 3)]);
    Fixture {
        engine,
        outer,
        home,
        token: callback.token,
        frozen_outer,
        cold_state,
    }
}

#[test]
fn cold_compile_preserves_cpu_and_requires_explicit_installation_and_selection() {
    let mut f = fixture(true);
    let before = observe(&f, &[]);
    let unit = compile_cold(&mut f, &[(COLD, 3)], &[]);
    assert_ne!(unit, f.home);
    assert_ne!(unit, f.outer);
    let after_compile = observe(&f, &[]);
    assert_eq!(after_compile.arena, before.arena);
    assert_eq!(after_compile.modules, before.modules);
    assert_eq!(ram(&f.engine), before.ram);
    assert_eq!(f.engine.generation(), before.generation);
    assert_eq!(f.engine.artifact_bytes().unwrap(), before.artifact.unwrap());
    assert_eq!(f.engine.lookup_resident(COLD).unwrap().get(), unit);
    assert_eq!(f.engine.guard_resident(KEY, f.home), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, unit), Err(busy()));
    assert_eq!(f.engine.lookup_installed_resident(KEY, COLD), Err(busy()));
    reject(
        &mut f,
        &[unit],
        HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(COLD),
        }),
        |f| {
            f.engine
                .select_resident_callback_unit(KEY, f.home, f.token, unit)
        },
    );
    reject(&mut f, &[unit], busy(), |f| {
        f.engine.compile_resident_with_gates(0, 0)
    });
    reject(&mut f, &[unit], busy(), |f| {
        f.engine
            .acknowledge_resident_installation(KEY, unit, u32::MAX)
    });

    let before_ack = observe(&f, &[unit]);
    assert_eq!(
        ack(&mut f, unit, 2),
        Ok(ResidentInstallation {
            unit_id: unit,
            slot: 2
        })
    );
    let after_ack = observe(&f, &[unit]);
    assert_eq!(after_ack.arena, before_ack.arena);
    assert_eq!(after_ack.ram, before_ack.ram);
    assert_eq!(after_ack.modules, before_ack.modules);
    assert_eq!(f.engine.guard_resident(KEY, unit), Err(busy()));
    assert_eq!(f.engine.lookup_installed_resident(KEY, COLD), Err(busy()));
    assert_eq!(
        ack(&mut f, unit, 2),
        Ok(ResidentInstallation {
            unit_id: unit,
            slot: 2
        })
    );
    let before_select = f.engine.arena().to_vec();
    f.engine
        .select_resident_callback_unit(KEY, f.home, f.token, unit)
        .unwrap();
    assert_eq!(f.engine.arena(), before_select);
    assert_eq!(ram(&f.engine), before.ram);
    assert_eq!(f.engine.guard_resident(KEY, unit), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.home), Err(busy()));
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, COLD),
        Ok(ResidentInstallation {
            unit_id: unit,
            slot: 2
        })
    );
    f.engine.abort_callback(KEY, f.token).unwrap();
    assert_eq!(&f.engine.arena()[..EXIT_OFFSET + EXIT_SIZE], f.frozen_outer);
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, COLD),
        Ok(ResidentInstallation {
            unit_id: unit,
            slot: 2
        })
    );
    assert_eq!(ram(&f.engine), before.ram);
    reject_both(&mut f, invalid_token());
    f.engine
        .complete_resident_call(KEY, f.outer, 1, 47)
        .unwrap();
    let caller = decode_state(&f.engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!(
        (caller.eip, caller.registers[0], caller.registers[4]),
        (0x3005, 47, 0x9000)
    );
    f.engine.close();
    assert_eq!(
        f.engine.compile_resident_callback_unit(0, 0, 0, 0, 0),
        Err(HostError::Closed)
    );
    assert_eq!(
        f.engine
            .acknowledge_resident_callback_installation(0, 0, 0, 0, 0),
        Err(HostError::Closed)
    );
}

fn busy() -> HostError {
    HostError::Call(CallError::Busy)
}
fn invalid_token() -> HostError {
    HostError::Call(CallError::InvalidToken)
}
fn invalid_stop() -> HostError {
    HostError::Call(CallError::InvalidStop)
}
fn cancelled() -> HostError {
    HostError::Call(CallError::Cancelled)
}
fn stale() -> HostError {
    HostError::Resident(RegistryError::CodeInvalidated)
}

fn ram(engine: &EngineInstance) -> Vec<Vec<u8>> {
    PAGES
        .into_iter()
        .map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct Module {
    id: u64,
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    ram: Vec<Vec<u8>>,
    generation: u32,
    artifact: Result<Vec<u8>, HostError>,
    modules: Vec<Module>,
    lookups: Vec<Result<u64, HostError>>,
    installations: Vec<Result<ResidentInstallation, HostError>>,
}

fn observe(f: &Fixture, extra_units: &[u64]) -> Observation {
    let pcs = [OUTER, HOME, RETURN, COLD, 0x6010, 0x6020, 0x6200];
    Observation {
        arena: f.engine.arena().to_vec(),
        ram: ram(&f.engine),
        generation: f.engine.generation(),
        artifact: f.engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        modules: [f.outer, f.home]
            .into_iter()
            .chain(extra_units.iter().copied())
            .map(|id| {
                let bytes = f.engine.resident_bytes(id);
                Module {
                    id,
                    pointer: bytes.as_ref().ok().map(|bytes| bytes.as_ptr() as usize),
                    bytes: bytes.map(|bytes| bytes.to_vec()),
                    guard: f.engine.guard_resident(KEY, id),
                }
            })
            .collect(),
        lookups: pcs
            .into_iter()
            .map(|pc| f.engine.lookup_resident(pc).map(|id| id.get()))
            .collect(),
        installations: pcs
            .into_iter()
            .map(|pc| f.engine.lookup_installed_resident(KEY, pc))
            .collect(),
    }
}

fn reject<T: std::fmt::Debug + PartialEq>(
    f: &mut Fixture,
    units: &[u64],
    expected: HostError,
    action: impl FnOnce(&mut Fixture) -> Result<T, HostError>,
) {
    let before = observe(f, units);
    let snapshots = PAGES.map(|pc| {
        f.engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(pc), 4096)
            .unwrap()
    });
    assert_eq!(action(f), Err(expected));
    assert_eq!(observe(f, units), before);
    for snapshot in &snapshots {
        assert!(f.engine.memory().unwrap().is_code_current(snapshot));
    }
}

fn reject_both(f: &mut Fixture, expected: HostError) {
    reject(f, &[], expected, |f| {
        f.engine
            .compile_resident_callback_unit(KEY, f.home, f.token, 0, 0)
    });
    reject(f, &[], expected, |f| {
        f.engine
            .acknowledge_resident_callback_installation(KEY, f.home, f.token, 0, u32::MAX)
    });
}

fn compile_cold(f: &mut Fixture, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    let pairs: Vec<_> = blocks.iter().chain(gates).copied().collect();
    descriptors(&mut f.engine, &pairs);
    f.engine
        .compile_resident_callback_unit(
            KEY,
            f.home,
            f.token,
            blocks.len() as u32,
            gates.len() as u32,
        )
        .unwrap()
        .get()
}

fn ack(f: &mut Fixture, unit: u64, slot: u32) -> Result<ResidentInstallation, HostError> {
    f.engine
        .acknowledge_resident_callback_installation(KEY, f.home, f.token, unit, slot)
}

#[test]
fn admission_uses_private_identity_before_stop_and_request_fields() {
    let mut f = fixture(true);
    for (key, home, token, expected) in [
        (KEY ^ (1 << 32), f.home, f.token, HostError::InvalidArtifact),
        (
            KEY,
            f.home ^ (1 << 32),
            f.token,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (KEY, f.home, 0, invalid_token()),
        (KEY, f.home, 1, invalid_token()),
        (KEY, f.outer, f.token, invalid_token()),
    ] {
        reject(&mut f, &[], expected, |f| {
            f.engine
                .compile_resident_callback_unit(key, home, token, 0, 0)
        });
        reject(&mut f, &[], expected, |f| {
            f.engine
                .acknowledge_resident_callback_installation(key, home, token, 0, u32::MAX)
        });
    }
    f.engine.arena_mut().unwrap()[STATE_OFFSET] ^= 1;
    reject_both(&mut f, invalid_stop());
    stop(&mut f.engine, f.cold_state, ExitReason::Budget);
    reject_both(&mut f, invalid_stop());
    encode_exit(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut f.engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    reject_both(&mut f, invalid_stop());
    f.engine.arena_mut().unwrap()[EXIT_OFFSET + 4..EXIT_OFFSET + 6]
        .copy_from_slice(&2_u16.to_le_bytes());
    reject_both(&mut f, invalid_stop());
    stop(&mut f.engine, f.cold_state, ExitReason::NeedCode);
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    reject_both(&mut f, cancelled());
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
    let unit = compile_cold(&mut f, &[(COLD, 3)], &[]);
    assert!(f.engine.resident_bytes(unit).is_ok());
}

#[test]
fn unarmed_and_replacement_callbacks_cannot_use_cold_front_doors() {
    let mut f = fixture(false);
    f.engine.arena_mut().unwrap()[STATE_OFFSET] ^= 1;
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    reject_both(&mut f, busy());
    f.engine.abort_callback(KEY, f.token).unwrap();
    f.engine.abandon_call(KEY, 1).unwrap();
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
    let mut state = f.cold_state;
    state.eip = HOME_GATE;
    state.registers[4] = 0x8ffc;
    stop(&mut f.engine, state, ExitReason::Gate { id: 19 });
    let generation = f.engine.generation();
    let call = f
        .engine
        .capture_call(KEY, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    let replacement = f
        .engine
        .begin_callback(KEY, generation, call.token, HOME, RETURN, 18, &[])
        .unwrap();
    f.token = replacement.token;
    reject_both(&mut f, invalid_token());
    f.engine.abort_callback(KEY, f.token).unwrap();
    f.engine.abandon_call(KEY, call.token).unwrap();
    reject_both(&mut f, invalid_token());
}

#[test]
fn stopped_entry_and_compile_failures_do_not_publish_or_consume_capacity() {
    let mut f = fixture(true);
    for (blocks, gates, expected) in [
        (vec![(0x6010, 1)], vec![], HostError::InvalidRequest),
        (vec![(COLD, 1)], vec![(COLD, 31)], HostError::InvalidRequest),
        (
            vec![(COLD, 0)],
            vec![],
            HostError::Resident(RegistryError::Compile(
                ring3_engine::cpu::dbt::CompileError::InvalidBlocks,
            )),
        ),
    ] {
        let pairs: Vec<_> = blocks.iter().chain(&gates).copied().collect();
        descriptors(&mut f.engine, &pairs);
        reject(&mut f, &[], expected, |f| {
            f.engine.compile_resident_callback_unit(
                KEY,
                f.home,
                f.token,
                blocks.len() as u32,
                gates.len() as u32,
            )
        });
    }
    for (count, gates) in [(0, 0), (9, 0), (1, 2)] {
        reject(&mut f, &[], HostError::InvalidRequest, |f| {
            f.engine
                .compile_resident_callback_unit(KEY, f.home, f.token, count, gates)
        });
    }
    f.engine.write32(COLD, 0x9090_0b0f).unwrap();
    descriptors(&mut f.engine, &[(COLD, 2)]);
    let before = observe(&f, &[]);
    assert!(matches!(
        f.engine
            .compile_resident_callback_unit(KEY, f.home, f.token, 1, 0),
        Err(HostError::Resident(RegistryError::Compile(_)))
    ));
    assert_eq!(observe(&f, &[]), before);
    f.engine.write32(COLD, 0x0000_eb90).unwrap();
    let unit = compile_cold(&mut f, &[(COLD, 3)], &[]);
    descriptors(&mut f.engine, &[(COLD, 3)]);
    reject(
        &mut f,
        &[unit],
        HostError::Resident(RegistryError::InstructionOverlap {
            pc: GuestAddress(COLD),
        }),
        |f| {
            f.engine
                .compile_resident_callback_unit(KEY, f.home, f.token, 1, 0)
        },
    );
    for index in 1..=5 {
        let mut state = f.cold_state;
        state.eip = 0x6300 + index * 0x10;
        stop(&mut f.engine, state, ExitReason::NeedCode);
        compile_cold(&mut f, &[(state.eip, 1)], &[]);
    }
    let mut state = f.cold_state;
    state.eip = 0x6400;
    stop(&mut f.engine, state, ExitReason::NeedCode);
    descriptors(&mut f.engine, &[(state.eip, 1)]);
    reject(
        &mut f,
        &[unit],
        HostError::Resident(RegistryError::UnitCapacity),
        |f| {
            f.engine
                .compile_resident_callback_unit(KEY, f.home, f.token, 1, 0)
        },
    );
    assert_eq!(
        f.engine.lookup_resident(state.eip),
        Err(HostError::Resident(RegistryError::NotFound {
            pc: GuestAddress(state.eip)
        }))
    );
}

#[test]
fn acknowledgement_checks_current_target_and_retained_slot_ownership() {
    let mut f = fixture(true);
    let unit = compile_cold(&mut f, &[(COLD, 3)], &[]);
    for (target, slot, expected) in [
        (
            unit ^ (1 << 32),
            2,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (f.outer, 2, HostError::InvalidRequest),
        (f.home, 2, HostError::InvalidRequest),
        (unit, 0, HostError::InvalidRequest),
        (unit, 8, HostError::InvalidRequest),
    ] {
        reject(&mut f, &[unit], expected, |f| ack(f, target, slot));
    }
    ack(&mut f, unit, 2).unwrap();
    reject(&mut f, &[unit], HostError::InvalidRequest, |f| {
        ack(f, unit, 3)
    });
    assert_eq!(
        ack(&mut f, unit, 2),
        Ok(ResidentInstallation {
            unit_id: unit,
            slot: 2
        })
    );
    f.engine.write32(COLD, 0x0000_eb90).unwrap();
    reject(&mut f, &[unit], stale(), |f| ack(f, unit, 2));
    let replacement = compile_cold(&mut f, &[(COLD, 3)], &[]);
    assert_ne!(replacement, unit);
    reject(
        &mut f,
        &[unit, replacement],
        HostError::InvalidRequest,
        |f| ack(f, replacement, 2),
    );
    ack(&mut f, replacement, 3).unwrap();
    f.engine
        .select_resident_callback_unit(KEY, f.home, f.token, replacement)
        .unwrap();
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, COLD),
        Ok(ResidentInstallation {
            unit_id: replacement,
            slot: 3
        })
    );
}

#[test]
fn retained_home_outer_and_active_staleness_precede_later_request_fields() {
    for owner in [HOME, OUTER, COLD] {
        let mut f = fixture(true);
        let active = compile_cold(&mut f, &[(COLD, 3)], &[]);
        ack(&mut f, active, 2).unwrap();
        f.engine
            .select_resident_callback_unit(KEY, f.home, f.token, active)
            .unwrap();
        f.engine.write32(owner, 0x9090_9090).unwrap();
        f.engine.arena_mut().unwrap()[STATE_OFFSET] ^= 1;
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, &[active], stale(), |f| {
            f.engine
                .compile_resident_callback_unit(KEY, f.home, f.token, 0, 0)
        });
        reject(&mut f, &[active], stale(), |f| {
            f.engine
                .acknowledge_resident_callback_installation(KEY, f.home, f.token, 0, u32::MAX)
        });
        f.engine.abort_callback(KEY, f.token).unwrap();
        assert_eq!(&f.engine.arena()[..EXIT_OFFSET + EXIT_SIZE], f.frozen_outer);
    }
}

#[test]
fn pending_inner_call_blocks_cold_operations_and_abort_preserves_installed_code() {
    let mut f = fixture(true);
    let active = compile_cold(
        &mut f,
        &[(COLD, 3), (0x6010, 1), (0x6200, 2)],
        &[(0x6200, 19)],
    );
    ack(&mut f, active, 2).unwrap();
    f.engine
        .select_resident_callback_unit(KEY, f.home, f.token, active)
        .unwrap();
    f.engine.write32(0x8ff4, 0x6010).unwrap();
    let mut inner = f.cold_state;
    inner.eip = 0x6200;
    inner.registers[4] = 0x8ff4;
    stop(&mut f.engine, inner, ExitReason::Gate { id: 19 });
    let captured = f
        .engine
        .capture_active_resident_callback_call(KEY, active, f.token, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(captured.token, 3);
    f.engine.arena_mut().unwrap()[STATE_OFFSET] ^= 1;
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
        .copy_from_slice(&1_u32.to_le_bytes());
    reject_both(&mut f, busy());
    f.engine.write32(0x7000, 0xaabb_ccdd).unwrap();
    f.engine.write32(0x8ff4, 0xdead_beef).unwrap();
    let committed_ram = ram(&f.engine);
    f.engine.abort_callback(KEY, f.token).unwrap();
    assert_eq!(&f.engine.arena()[..EXIT_OFFSET + EXIT_SIZE], f.frozen_outer);
    assert_eq!(ram(&f.engine), committed_ram);
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, COLD),
        Ok(ResidentInstallation {
            unit_id: active,
            slot: 2
        })
    );
    reject_both(&mut f, invalid_token());
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
    assert_eq!(
        f.engine
            .complete_active_resident_callback_call(KEY, active, f.token, captured.token, 44),
        Err(invalid_token())
    );
    f.engine
        .complete_resident_call(KEY, f.outer, 1, 47)
        .unwrap();
    let continued = decode_state(&f.engine.arena()[..STATE_SIZE]).unwrap();
    assert_eq!((continued.eip, continued.registers[0]), (0x3005, 47));
}
