use ring3_engine::{
    abi::x86::{encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const OUTER: u32 = 0x4000;
const HOME: u32 = 0x5000;
const RETURN: u32 = 0x5100;
const TARGET: u32 = 0x6000;
const PAGES: [u32; 5] = [0x3000, OUTER, HOME, TARGET, 0x8000];

fn descriptors(engine: &mut EngineInstance, pairs: &[(u32, u32)]) {
    for (index, (pc, value)) in pairs.iter().enumerate() {
        let start = 140 + index * 8;
        let arena = engine.arena_mut().unwrap();
        arena[start..start + 4].copy_from_slice(&pc.to_le_bytes());
        arena[start + 4..start + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn read_ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

#[test]
fn selection_preserves_public_bytes_and_moves_only_authorized_execution() {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for pc in PAGES {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (OUTER, &[0x0f, 0x0b][..]),
        (HOME, &[0x90][..]),
        (RETURN, &[0x0f, 0x0b][..]),
        (TARGET, &[0x90][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.write32(0x8ffc, 0x3005).unwrap();
    descriptors(&mut engine, &[(OUTER, 2), (OUTER, 17)]);
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    descriptors(&mut engine, &[(HOME, 1), (RETURN, 2), (RETURN, 18)]);
    let home = engine.compile_resident_with_gates(2, 1).unwrap().get();
    descriptors(&mut engine, &[(TARGET, 1)]);
    let target = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, home, 1)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, target, 2)
        .unwrap();
    let frozen = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: OUTER,
        eflags: 0xcd7,
    };
    // typed native stops only; this test does not execute guest instructions.
    encode_state(&frozen, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let frozen_bytes = engine.arena()[..96].to_vec();
    let outer_call = engine
        .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(outer_call.token, 1);
    let admitted = engine
        .begin_resident_callback(KEY, outer, home, 1, HOME, RETURN, 18, &[])
        .unwrap();
    assert_eq!(
        (admitted.token, admitted.outer_token, admitted.entry_esp),
        (2, 1, 0x8ff8)
    );
    engine.authorize_resident_callback(KEY, home, 2).unwrap();
    let receipt = words(&[
        u32::from_le_bytes(*b"R3RC"),
        0x10001,
        72,
        0,
        2,
        1,
        1,
        0,
        HOME,
        0x8ff8,
        RETURN,
        18,
        0,
        0,
        outer as u32,
        (outer >> 32) as u32,
        home as u32,
        (home >> 32) as u32,
    ]);
    assert_eq!(&engine.arena()[140..212], receipt);
    let mut stopped = frozen;
    stopped.eip = TARGET;
    stopped.registers[0] = 13;
    stopped.registers[4] = 0x8ff8;
    encode_state(&stopped, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 2,
            reason: ExitReason::NeedCode,
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    let before = engine.arena().to_vec();
    let stack = read_ram(&engine, 0x8fc0, 64);
    let busy = HostError::Call(CallError::Busy);
    assert_eq!(engine.guard_resident(KEY, home), Ok(()));
    assert_eq!(engine.guard_resident(KEY, target), Err(busy));
    assert_eq!(
        engine.select_resident_callback_unit(KEY, home, 2, target),
        Ok(())
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(read_ram(&engine, 0x8fc0, 64), stack);
    assert_eq!(engine.guard_resident(KEY, target), Ok(()));
    assert_eq!(engine.guard_resident(KEY, home), Err(busy));
    assert_eq!(engine.guard_resident(KEY, outer), Err(busy));
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, TARGET)
            .unwrap()
            .unit_id,
        target
    );
    assert_eq!(engine.lookup_installed_resident(KEY, HOME), Err(busy));
    assert_eq!(engine.finish_resident_callback(KEY, home, 2), Err(busy));
    assert_eq!(&engine.arena()[140..212], receipt);
    assert_eq!(engine.arena(), before);
    engine.abort_callback(KEY, 2).unwrap();
    assert_eq!(&engine.arena()[..96], frozen_bytes);
    assert_eq!(read_ram(&engine, 0x8fc0, 64), stack);
    engine.complete_resident_call(KEY, outer, 1, 18).unwrap();
    let mut continued = frozen;
    continued.eip = 0x3005;
    continued.registers[0] = 18;
    continued.registers[4] = 0x9000;
    let mut continued_bytes = [0; 56];
    encode_state(&continued, &mut continued_bytes).unwrap();
    assert_eq!(&engine.arena()[..56], continued_bytes);
}

use ring3_engine::{
    cpu::dbt::RegistryError,
    process::{ResidentInstallation, StoreCompletion},
};

const INNER: u32 = 0x5200;
const RESUME: u32 = 0x5010;
const TARGET_GATE: u32 = 0x6100;
const OTHER: u32 = 0x7000;
const ALL_PAGES: [u32; 6] = [0x3000, OUTER, HOME, TARGET, OTHER, 0x8000];

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    home: u64,
    target: u64,
    other: u64,
    generation: u32,
}

fn fixture(home_ack: bool, target_ack: bool, same_home: bool) -> Fixture {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for pc in ALL_PAGES {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (OUTER, &[0x0f, 0x0b][..]),
        (HOME, &[0x90][..]),
        (RESUME, &[0x90][..]),
        (RETURN, &[0x0f, 0x0b][..]),
        (INNER, &[0x0f, 0x0b][..]),
        (TARGET, &[0x90][..]),
        (TARGET_GATE, &[0x0f, 0x0b][..]),
        (OTHER, &[0x90][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.write32(0x8ffc, 0x3005).unwrap();
    let all = [
        (OUTER, 2),
        (HOME, 1),
        (RESUME, 1),
        (RETURN, 2),
        (INNER, 2),
        (OUTER, 17),
        (RETURN, 18),
        (INNER, 19),
    ];
    descriptors(&mut engine, &all);
    let generation = engine.compile_with_gates(5, 3).unwrap();
    let (outer, home) = if same_home {
        descriptors(&mut engine, &all);
        let id = engine.compile_resident_with_gates(5, 3).unwrap().get();
        (id, id)
    } else {
        descriptors(&mut engine, &[(OUTER, 2), (OUTER, 17)]);
        let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
        descriptors(
            &mut engine,
            &[
                (HOME, 1),
                (RESUME, 1),
                (RETURN, 2),
                (INNER, 2),
                (RETURN, 18),
                (INNER, 19),
            ],
        );
        (
            outer,
            engine.compile_resident_with_gates(4, 2).unwrap().get(),
        )
    };
    descriptors(
        &mut engine,
        &[(TARGET, 1), (TARGET_GATE, 2), (TARGET_GATE, 20)],
    );
    let target = engine.compile_resident_with_gates(2, 1).unwrap().get();
    descriptors(&mut engine, &[(OTHER, 1)]);
    let other = engine.compile_resident(1).unwrap().get();
    if home_ack {
        engine
            .acknowledge_resident_installation(KEY, home, 1)
            .unwrap();
    }
    if target_ack {
        engine
            .acknowledge_resident_installation(KEY, target, 2)
            .unwrap();
    }
    Fixture {
        engine,
        outer,
        home,
        target,
        other,
        generation,
    }
}

fn frozen_state() -> State32 {
    State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: OUTER,
        eflags: 0xcd7,
    }
}

fn state_words(state: &State32) -> Vec<u8> {
    let mut fields = vec![u32::from_le_bytes(*b"R3ST"), 0x10001, 56, 0];
    fields.extend(state.registers);
    fields.extend([state.eip, state.eflags]);
    words(&fields)
}

fn exit_words(version: u32, reason: u32, retired: u32, detail: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3EX"),
        0x10000 | version,
        40,
        0,
        reason,
        retired,
        detail,
        0,
        0,
        0,
    ])
}

fn stop(f: &mut Fixture, pc: u32, version: u32, reason: u32, retired: u32, detail: u32) {
    let mut state = frozen_state();
    state.eip = pc;
    state.registers[0] = 13;
    state.registers[4] = 0x8ff8;
    let arena = f.engine.arena_mut().unwrap();
    arena[..56].copy_from_slice(&state_words(&state));
    arena[56..96].copy_from_slice(&exit_words(version, reason, retired, detail));
}

fn admit(f: &mut Fixture, replacement: bool, armed: bool) {
    let arena = f.engine.arena_mut().unwrap();
    arena[..56].copy_from_slice(&state_words(&frozen_state()));
    arena[56..96].copy_from_slice(&exit_words(3, 8, 1, 17));
    arena[96..100].fill(0);
    let token = if replacement {
        f.engine
            .capture_call(KEY, f.generation, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    } else {
        f.engine
            .capture_resident_call(KEY, f.outer, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    };
    assert_eq!(token, 1);
    if replacement {
        let record = f
            .engine
            .begin_callback(KEY, f.generation, 1, HOME, RETURN, 18, &[])
            .unwrap();
        assert_eq!(record.token, 2);
    } else {
        let record = f
            .engine
            .begin_resident_callback(KEY, f.outer, f.home, 1, HOME, RETURN, 18, &[])
            .unwrap();
        assert_eq!(
            (record.token, record.outer_token, record.entry_esp),
            (2, 1, 0x8ff8)
        );
        assert_eq!(
            &f.engine.arena()[140..212],
            words(&[
                u32::from_le_bytes(*b"R3RC"),
                0x10001,
                72,
                0,
                2,
                1,
                1,
                0,
                HOME,
                0x8ff8,
                RETURN,
                18,
                0,
                0,
                f.outer as u32,
                (f.outer >> 32) as u32,
                f.home as u32,
                (f.home >> 32) as u32,
            ])
        );
        if armed {
            f.engine
                .authorize_resident_callback(KEY, f.home, 2)
                .unwrap();
        }
    }
}

fn ready() -> Fixture {
    let mut f = fixture(true, true, false);
    admit(&mut f, false, true);
    stop(&mut f, TARGET, 3, 3, 2, 0);
    f
}

fn call(error: CallError) -> HostError {
    HostError::Call(error)
}
fn stale() -> HostError {
    HostError::Resident(RegistryError::CodeInvalidated)
}
fn invalid_unit() -> HostError {
    HostError::Resident(RegistryError::InvalidUnit)
}
fn missing(pc: u32) -> HostError {
    HostError::Resident(RegistryError::NotFound {
        pc: GuestAddress(pc),
    })
}

type Artifact = Result<(Vec<u8>, usize), HostError>;
#[derive(Debug, PartialEq, Eq)]
struct Storage {
    arena: Vec<u8>,
    address: usize,
    generation: u32,
    modules: Vec<Artifact>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    versions: Vec<Result<String, HostError>>,
    logical: Vec<Result<u64, HostError>>,
}

fn storage(f: &Fixture) -> Storage {
    let copy = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Storage {
        arena: f.engine.arena().to_vec(),
        address: f.engine.arena_address(),
        generation: f.engine.generation(),
        modules: vec![
            f.engine.dispatcher_bytes(KEY).map(copy),
            f.engine.artifact_bytes().map(copy),
            f.engine.resident_bytes(f.outer).map(copy),
            f.engine.resident_bytes(f.home).map(copy),
            f.engine.resident_bytes(f.target).map(copy),
            f.engine.resident_bytes(f.other).map(copy),
        ],
        ram: ALL_PAGES
            .iter()
            .map(|&pc| {
                let mut bytes = vec![0; 4096];
                f.engine
                    .memory()?
                    .read(GuestAddress(pc), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .collect(),
        // derived Debug includes identity, first page, and all mapping/content versions.
        versions: ALL_PAGES
            .iter()
            .map(|&pc| {
                f.engine
                    .memory()?
                    .snapshot_code(GuestAddress(pc), 4096)
                    .map(|snapshot| format!("{snapshot:?}"))
                    .map_err(HostError::Memory)
            })
            .collect(),
        logical: [
            OUTER,
            HOME,
            RESUME,
            RETURN,
            INNER,
            TARGET,
            TARGET_GATE,
            OTHER,
        ]
        .map(|pc| f.engine.lookup_resident(pc).map(|id| id.get()))
        .to_vec(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    storage: Storage,
    guards: Vec<Result<(), HostError>>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
}
fn observe(f: &Fixture) -> Observation {
    Observation {
        storage: storage(f),
        guards: vec![
            f.engine.guard_dispatch_entry(KEY),
            f.engine.guard(KEY, f.generation),
            f.engine.guard_resident(KEY, f.outer),
            f.engine.guard_resident(KEY, f.home),
            f.engine.guard_resident(KEY, f.target),
            f.engine.guard_resident(KEY, f.other),
        ],
        installed: [OUTER, HOME, RETURN, TARGET, TARGET_GATE, OTHER]
            .map(|pc| f.engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
    }
}

fn unchanged<T: std::fmt::Debug + PartialEq>(
    f: &mut Fixture,
    expected: Result<T, HostError>,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = observe(f);
    assert_eq!(operation(&mut f.engine), expected);
    assert_eq!(observe(f), before);
}

fn select(f: &mut Fixture, target: u64) {
    let before = storage(f);
    f.engine
        .select_resident_callback_unit(KEY, f.home, 2, target)
        .unwrap();
    assert_eq!(storage(f), before);
    assert_eq!(f.engine.guard_resident(KEY, target), Ok(()));
}

fn arena_only(f: &Fixture, mut before: Storage, expected: Vec<u8>) {
    before.arena = expected;
    assert_eq!(storage(f), before);
}

fn restored_by_abort(f: &mut Fixture) {
    let before = storage(f);
    f.engine.abort_callback(KEY, 2).unwrap();
    let mut expected = before.arena.clone();
    expected[..56].copy_from_slice(&state_words(&frozen_state()));
    expected[56..96].copy_from_slice(&exit_words(3, 8, 1, 17));
    let after = storage(f);
    assert_eq!(after.arena, expected);
    assert_eq!(after.address, before.address);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.modules, before.modules);
    assert_eq!(after.ram, before.ram);
    assert_eq!(after.versions, before.versions);
    assert_eq!(after.logical, before.logical);
}

#[test]
fn selector_priorities_canonical_versions_acknowledgement_and_idempotence() {
    // typed stop inputs cover all exit versions and diagnostic retirement, not guest history.
    for version in [1, 2, 3] {
        for retired in [0, u32::MAX] {
            let mut f = fixture(false, true, false);
            admit(&mut f, false, true);
            stop(&mut f, TARGET, version, 3, retired, 0);
            let (home, target) = (f.home, f.target);
            assert_eq!(
                f.engine.lookup_installed_resident(KEY, HOME),
                Err(missing(HOME))
            );
            select(&mut f, target);
            unchanged(&mut f, Ok(()), |e| {
                e.select_resident_callback_unit(KEY, home, 2, target)
            });
            f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
            unchanged(&mut f, Err(call(CallError::Cancelled)), |e| {
                e.select_resident_callback_unit(KEY, home, 2, target)
            });
            stop(&mut f, HOME, version, 3, retired, 0);
            unchanged(&mut f, Err(missing(HOME)), |e| {
                e.select_resident_callback_unit(KEY, home, 2, home)
            });
            // only the newly selected target requires ack; outer and previous home never had one.
            restored_by_abort(&mut f);
            f.engine.arena_mut().unwrap()[96..100].fill(0);
            let next = f
                .engine
                .begin_resident_callback(KEY, f.outer, home, 1, HOME, RETURN, 18, &[])
                .unwrap();
            assert_eq!((next.token, next.outer_token), (3, 1));
            assert_eq!(
                f.engine.guard_resident(KEY, target),
                Err(call(CallError::Busy))
            );
        }
    }
    let mut f = ready();
    let (home, target, outer, other) = (f.home, f.target, f.outer, f.other);
    let invalid = invalid_unit();
    for (key, id, token, next, error) in [
        (KEY ^ 1, home, 2, target, HostError::InvalidArtifact),
        (KEY ^ (1 << 32), home, 2, target, HostError::InvalidArtifact),
        (KEY, 0, 0, target, invalid),
        (KEY, home ^ (1 << 32), 0, target, invalid),
        (KEY, target, 2, target, call(CallError::InvalidToken)),
        (KEY, home, 0, target, call(CallError::InvalidToken)),
        (KEY, home, 1, target, call(CallError::InvalidToken)),
        (KEY, home, 3, target, call(CallError::InvalidToken)),
        (KEY, home, 2, 0, invalid),
        (KEY, home, 2, target ^ (1 << 32), invalid),
        (KEY, home, 2, outer, HostError::InvalidRequest),
    ] {
        unchanged(&mut f, Err(error), |e| {
            e.select_resident_callback_unit(key, id, token, next)
        });
    }
    for (offset, value) in [
        (0, 0),
        (4, 0x10002),
        (8, 55),
        (12, 1),
        (52, u32::MAX),
        (56, 0),
        (60, 0x10004),
        (64, 39),
        (68, 1),
        (72, 1),
        (80, 1),
        (84, 1),
    ] {
        stop(&mut f, TARGET, 3, 3, 2, 0);
        f.engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        unchanged(&mut f, Err(call(CallError::InvalidStop)), |e| {
            e.select_resident_callback_unit(KEY, home, 2, target)
        });
    }
    for (pc, next) in [
        (TARGET + 1, target),
        (TARGET_GATE, target),
        (INNER, home),
        (OTHER, target),
    ] {
        stop(&mut f, pc, 3, 3, 2, 0);
        unchanged(&mut f, Err(call(CallError::InvalidStop)), |e| {
            e.select_resident_callback_unit(KEY, home, 2, next)
        });
    }
    stop(&mut f, OTHER, 3, 3, 2, 0);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    unchanged(&mut f, Err(missing(OTHER)), |e| {
        e.select_resident_callback_unit(KEY, home, 2, other)
    });
    f.engine.arena_mut().unwrap()[..56].fill(0);
    unchanged(&mut f, Err(invalid), |e| {
        e.select_resident_callback_unit(KEY, home, 2, 0)
    });
    unchanged(&mut f, Err(HostError::InvalidRequest), |e| {
        e.select_resident_callback_unit(KEY, home, 2, outer)
    });
    unchanged(&mut f, Err(call(CallError::InvalidStop)), |e| {
        e.select_resident_callback_unit(KEY, home, 2, other)
    });
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    stop(&mut f, RETURN, 2, 3, 0, 0);
    select(&mut f, home);
    let mut absent = fixture(true, true, false);
    let (home, target) = (absent.home, absent.target);
    unchanged(&mut absent, Err(call(CallError::InvalidToken)), |e| {
        e.select_resident_callback_unit(KEY, home, 2, target)
    });
    for replacement in [false, true] {
        let mut f = fixture(true, true, false);
        admit(&mut f, replacement, false);
        let (home, target) = (f.home, f.target);
        f.engine.write32(TARGET, 0x90).unwrap();
        f.engine.arena_mut().unwrap()[..56].fill(0);
        unchanged(
            &mut f,
            Err(call(if replacement {
                CallError::InvalidToken
            } else {
                CallError::Busy
            })),
            |e| e.select_resident_callback_unit(KEY, home, 2, target),
        );
    }
    let mut same = fixture(true, true, true);
    admit(&mut same, false, false);
    let (home, target) = (same.home, same.target);
    unchanged(&mut same, Err(call(CallError::Busy)), |e| {
        e.select_resident_callback_unit(KEY, home, 2, target)
    });
    unchanged(&mut same, Err(call(CallError::InvalidRequest)), |e| {
        e.authorize_resident_callback(KEY, home, 2)
    });
}

#[test]
fn active_unit_guards_home_ownership_and_inner_pending_remain_distinct() {
    let mut f = ready();
    let (home, target, outer, other, generation) =
        (f.home, f.target, f.outer, f.other, f.generation);
    select(&mut f, target);
    assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    for id in [home, outer, other] {
        unchanged(&mut f, Err(call(CallError::Busy)), |e| {
            e.guard_resident(KEY, id)
        });
    }
    unchanged(&mut f, Err(HostError::InvalidArtifact), |e| {
        e.guard_resident(KEY ^ (1 << 32), target)
    });
    unchanged(&mut f, Err(invalid_unit()), |e| {
        e.guard_resident(KEY, target ^ (1 << 32))
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.guard(KEY, generation)
    });
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, TARGET),
        Ok(ResidentInstallation {
            unit_id: target,
            slot: 2
        })
    );
    for pc in [HOME, RETURN, OTHER] {
        unchanged(&mut f, Err(call(CallError::Busy)), |e| {
            e.lookup_installed_resident(KEY, pc)
        });
    }
    unchanged(&mut f, Err(missing(0xdead)), |e| {
        e.lookup_installed_resident(KEY, 0xdead)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.capture_resident_callback_call_raw(KEY, home, 2, 0, 17)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.complete_resident_callback_call(KEY, home, 2, 0, 18)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.finish_resident_callback(KEY, home, 2)
    });
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.capture_resident_callback_call_raw(KEY, target, 2, 1, 0)
    });
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.complete_resident_callback_call(KEY, target, 2, 3, 18)
    });
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.finish_resident_callback(KEY, target, 2)
    });
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.finish_callback(KEY, generation, 2)
    });
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.resume_callback_code(KEY, generation, 2, 0, 0)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.compile_resident_with_gates(0, 0)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.compile_with_gates(0, 0)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.acknowledge_resident_installation(KEY, target, 9)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.begin_resident_callback(KEY, outer, home, 1, HOME, RETURN, 18, &[0; 17])
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.capture_resident_call_raw(KEY, home, 0, 17)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.complete_resident_call(KEY, outer, 1, 18)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.abandon_call(KEY, 1)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.store_resident32(KEY, home, 0x8f00, 0x1122_3344)
    });
    let before = storage(&f);
    let mut ram = before.ram.clone();
    ram[5].as_mut().unwrap()[0xf00..0xf04].copy_from_slice(&0x1122_3344_u32.to_le_bytes());
    let mut arena = before.arena.clone();
    arena[100..140].copy_from_slice(&words(&[
        u32::from_le_bytes(*b"R3MH"),
        0x10001,
        40,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]));
    assert_eq!(
        f.engine.store_resident32(KEY, target, 0x8f00, 0x1122_3344),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(storage(&f).ram, ram);
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(storage(&f).modules, before.modules);
    assert_eq!(f.engine.guard_resident(KEY, target), Ok(()));
    stop(&mut f, HOME, 3, 3, 2, 0);
    select(&mut f, home);
    f.engine.write32(0x8ff4, RESUME).unwrap();
    stop(&mut f, INNER, 3, 8, 3, 19);
    f.engine.arena_mut().unwrap()[32..36].copy_from_slice(&0x8ff4_u32.to_le_bytes());
    let before = storage(&f);
    let inner = f
        .engine
        .capture_resident_callback_call(KEY, home, 2, CallingConvention32::Stdcall, 0)
        .unwrap();
    assert_eq!(
        (inner.token, inner.return_pc, inner.stack_words),
        (3, RESUME, 0)
    );
    let mut expected = before.arena.clone();
    let mut fields = vec![
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        3,
        19,
        2,
        0,
        INNER,
        0x8ff4,
        RESUME,
        0,
    ];
    fields.extend([0; 16]);
    expected[140..252].copy_from_slice(&words(&fields));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(storage(&f).ram, before.ram);
    arena_only(&f, before, expected);
    let pending_cpu = f.engine.arena()[..96].to_vec();
    f.engine.write32(TARGET, 0x90).unwrap();
    f.engine.arena_mut().unwrap()[..96].fill(0xa5);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.select_resident_callback_unit(KEY, home, 2, target)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.guard_dispatch_entry(KEY)
    });
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.guard_resident(KEY, home)
    });
    unchanged(&mut f, Err(stale()), |e| e.guard_resident(KEY, target));
    unchanged(&mut f, Err(call(CallError::Busy)), |e| {
        e.finish_resident_callback(KEY, home, 2)
    });
    unchanged(&mut f, Err(call(CallError::StateChanged)), |e| {
        e.complete_resident_callback_call(KEY, home, 2, 3, 44)
    });
    f.engine.arena_mut().unwrap()[..96].copy_from_slice(&pending_cpu);
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    let before = storage(&f);
    f.engine
        .complete_resident_callback_call(KEY, home, 2, 3, 44)
        .unwrap();
    let mut state = frozen_state();
    state.eip = RESUME;
    state.registers[0] = 44;
    state.registers[4] = 0x8ff8;
    let mut expected = before.arena.clone();
    expected[..56].copy_from_slice(&state_words(&state));
    expected[56..96].copy_from_slice(&exit_words(3, 3, 0, 0));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(storage(&f).ram, before.ram);
    arena_only(&f, before, expected);
    unchanged(&mut f, Err(stale()), |e| {
        e.select_resident_callback_unit(KEY, home, 2, target)
    });
    stop(&mut f, RETURN, 3, 8, 0, 18);
    f.engine.arena_mut().unwrap()[16..20].copy_from_slice(&18_u32.to_le_bytes());
    f.engine.arena_mut().unwrap()[32..36].copy_from_slice(&0x8ffc_u32.to_le_bytes());
    let before = storage(&f);
    let result = f.engine.finish_resident_callback(KEY, home, 2).unwrap();
    assert_eq!(
        (
            result.token,
            result.outer_token,
            result.result,
            result.outer_unit_id,
            result.callback_unit_id
        ),
        (2, 1, 18, outer, home)
    );
    let mut expected = before.arena.clone();
    expected[..56].copy_from_slice(&state_words(&frozen_state()));
    expected[56..96].copy_from_slice(&exit_words(3, 8, 1, 17));
    expected[140..188].copy_from_slice(&words(&[
        u32::from_le_bytes(*b"R3RR"),
        0x10001,
        48,
        0,
        2,
        1,
        18,
        0,
        outer as u32,
        (outer >> 32) as u32,
        home as u32,
        (home >> 32) as u32,
    ]));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(storage(&f).ram, before.ram);
    arena_only(&f, before, expected);
    unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
        e.finish_resident_callback(KEY, home, 2)
    });
    let next = f
        .engine
        .begin_resident_callback(KEY, outer, home, 1, HOME, RETURN, 18, &[])
        .unwrap();
    assert_eq!((next.token, next.outer_token), (4, 1));
    assert_eq!(
        f.engine.guard_resident(KEY, home),
        Err(call(CallError::Busy))
    );
    f.engine.abort_callback(KEY, 4).unwrap();
    f.engine.complete_resident_call(KEY, outer, 1, 18).unwrap();
    let mut state = frozen_state();
    state.eip = 0x3005;
    state.registers[0] = 18;
    state.registers[4] = 0x9000;
    assert_eq!(&f.engine.arena()[..56], state_words(&state));
}

#[test]
fn stale_roles_committed_writes_neutral_abort_and_close_do_not_rebind() {
    for role in 0..4 {
        for mutation in 0..3 {
            let mut f = ready();
            let (home, target, outer) = (f.home, f.target, f.outer);
            if role != 3 {
                select(&mut f, target);
            }
            let pc = [HOME, OUTER, TARGET, TARGET][role];
            stop(&mut f, if role == 3 { TARGET } else { RETURN }, 2, 3, 2, 0);
            match mutation {
                0 => {
                    let value = u32::from_le_bytes(read_ram(&f.engine, pc, 4).try_into().unwrap());
                    f.engine.write32(pc, value).unwrap();
                }
                1 => f.engine.protect(pc, 1, 7).unwrap(),
                _ => {
                    f.engine.unmap(pc, 1).unwrap();
                    f.engine.map(pc, 1, 7).unwrap();
                }
            }
            f.engine.arena_mut().unwrap()[..96].fill(0xa5);
            f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
            let next = if role == 3 { target } else { home };
            unchanged(&mut f, Err(stale()), |e| {
                e.select_resident_callback_unit(KEY, home, 2, next)
            });
            unchanged(&mut f, if role == 3 { Ok(()) } else { Err(stale()) }, |e| {
                e.guard_dispatch_entry(KEY)
            });
            unchanged(&mut f, if role == 3 { Ok(()) } else { Err(stale()) }, |e| {
                e.guard_resident(KEY, if role == 3 { home } else { target })
            });
            // finder stays pure inspection and adds no parent/home currency check.
            unchanged(
                &mut f,
                if role < 2 {
                    Ok(ResidentInstallation {
                        unit_id: target,
                        slot: 2,
                    })
                } else {
                    Err(stale())
                },
                |e| e.lookup_installed_resident(KEY, TARGET),
            );
            unchanged(
                &mut f,
                if role == 0 {
                    Err(stale())
                } else if role == 3 {
                    Ok(ResidentInstallation {
                        unit_id: home,
                        slot: 1,
                    })
                } else {
                    Err(call(CallError::Busy))
                },
                |e| e.lookup_installed_resident(KEY, HOME),
            );
            unchanged(
                &mut f,
                Err(if role == 0 {
                    stale()
                } else {
                    call(CallError::InvalidToken)
                }),
                |e| e.select_resident_callback_unit(KEY, home, 0, next),
            );
            restored_by_abort(&mut f);
            unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
                e.abort_callback(KEY, 2)
            });
            if role == 1 {
                unchanged(&mut f, Err(stale()), |e| {
                    e.complete_resident_call(KEY, outer, 1, 18)
                });
            }
            f.engine.abandon_call(KEY, 1).unwrap();
            let arena = f.engine.arena().to_vec();
            f.engine.close();
            assert_eq!(f.engine.arena(), arena);
            unchanged(&mut f, Err(HostError::Closed), |e| {
                e.select_resident_callback_unit(KEY ^ 1, home, 0, next)
            });
            unchanged(&mut f, Err(HostError::Closed), |e| e.abort_callback(KEY, 2));
        }
    }
    for pc in [HOME, OUTER, TARGET] {
        let mut f = ready();
        let (home, target) = (f.home, f.target);
        select(&mut f, target);
        let before = storage(&f);
        let page = ALL_PAGES.iter().position(|&base| base == pc).unwrap();
        let old = u32::from_le_bytes(before.ram[page].as_ref().unwrap()[..4].try_into().unwrap());
        let value = old ^ 0x10000;
        let mut ram = before.ram.clone();
        ram[page].as_mut().unwrap()[..4].copy_from_slice(&value.to_le_bytes());
        let mut arena = before.arena.clone();
        arena[100..140].copy_from_slice(&words(&[
            u32::from_le_bytes(*b"R3MH"),
            0x10001,
            40,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ]));
        assert_eq!(
            f.engine.store_resident32(KEY, target, pc, value),
            Ok(if pc == TARGET {
                StoreCompletion::CodeInvalidated
            } else {
                StoreCompletion::Complete
            })
        );
        assert_eq!(storage(&f).ram, ram);
        assert_eq!(f.engine.arena(), arena);
        assert_ne!(storage(&f).versions, before.versions);
        unchanged(&mut f, Err(stale()), |e| e.guard_dispatch_entry(KEY));
        unchanged(&mut f, Err(stale()), |e| e.guard_resident(KEY, target));
        unchanged(&mut f, Err(stale()), |e| {
            e.select_resident_callback_unit(KEY, home, 2, home)
        });
        restored_by_abort(&mut f);
        assert_eq!(storage(&f).ram, ram);
    }
    // neutral abort consumes an inner pending token; generic abandon instead only drops that inner.
    for abandon_inner in [false, true] {
        let mut f = ready();
        let (home, target, outer) = (f.home, f.target, f.outer);
        select(&mut f, target);
        stop(&mut f, HOME, 3, 3, 2, 0);
        select(&mut f, home);
        f.engine.write32(0x8ff4, RESUME).unwrap();
        stop(&mut f, INNER, 3, 8, 0, 19);
        f.engine.arena_mut().unwrap()[32..36].copy_from_slice(&0x8ff4_u32.to_le_bytes());
        let inner = f
            .engine
            .capture_resident_callback_call(KEY, home, 2, CallingConvention32::Cdecl, 0)
            .unwrap();
        assert_eq!(inner.token, 3);
        unchanged(&mut f, Err(call(CallError::Busy)), |e| {
            e.select_resident_callback_unit(KEY, home, 2, target)
        });
        if abandon_inner {
            f.engine.abandon_call(KEY, 3).unwrap();
            unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
                e.complete_resident_callback_call(KEY, home, 2, 3, 18)
            });
            stop(&mut f, TARGET, 3, 3, 0, 0);
            select(&mut f, target);
        }
        restored_by_abort(&mut f);
        unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
            e.complete_resident_callback_call(KEY, home, 2, 3, 18)
        });
        let next = f
            .engine
            .begin_resident_callback(KEY, outer, home, 1, HOME, RETURN, 18, &[])
            .unwrap();
        assert_eq!((next.token, next.outer_token), (4, 1));
        unchanged(&mut f, Err(call(CallError::Busy)), |e| {
            e.select_resident_callback_unit(KEY, home, 4, target)
        });
        unchanged(&mut f, Err(call(CallError::InvalidToken)), |e| {
            e.select_resident_callback_unit(KEY, home, 2, target)
        });
        f.engine.authorize_resident_callback(KEY, home, 4).unwrap();
        assert_eq!(f.engine.guard_resident(KEY, home), Ok(()));
        assert_eq!(
            f.engine.guard_resident(KEY, target),
            Err(call(CallError::Busy))
        );
    }
    let mut unarmed = fixture(true, true, false);
    admit(&mut unarmed, false, false);
    let (home, target, outer) = (unarmed.home, unarmed.target, unarmed.outer);
    unarmed.engine.write32(OUTER, 0x0b0f).unwrap();
    unchanged(&mut unarmed, Err(stale()), |e| {
        e.select_resident_callback_unit(KEY, home, 2, target)
    });
    unchanged(&mut unarmed, Err(call(CallError::InvalidToken)), |e| {
        e.select_resident_callback_unit(KEY, home, 0, target)
    });
    restored_by_abort(&mut unarmed);
    unchanged(&mut unarmed, Err(stale()), |e| {
        e.complete_resident_call(KEY, outer, 1, 18)
    });
    unarmed.engine.abandon_call(KEY, 1).unwrap();
}
