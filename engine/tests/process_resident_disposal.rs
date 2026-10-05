use ring3_engine::{
    cpu::dbt::RegistryError,
    memory::GuestAddress,
    process::{EngineInstance, HostError},
};

const KEY: u64 = 0x1020_3040_5060_7080;

#[test]
fn current_unacknowledged_unit_can_be_discarded_without_invalidating_guest_code() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[140..143].copy_from_slice(&[0x90, 0xeb, 0]);
    engine.upload(0x1000, 3).unwrap();
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&0x1000_u32.to_le_bytes());
    engine.arena_mut().unwrap()[144..148].copy_from_slice(&3_u32.to_le_bytes());
    let old = engine.compile_resident(1).unwrap().get();
    let before_arena = engine.arena().to_vec();
    let before_address = engine.arena_address();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 4096)
        .unwrap();
    let mut before_ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut before_ram)
        .unwrap();

    engine.discard_unacknowledged_resident(KEY, old).unwrap();

    assert_eq!(engine.arena(), before_arena);
    assert_eq!(engine.arena_address(), before_address);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
    let mut after_ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1000), &mut after_ram)
        .unwrap();
    assert_eq!(after_ram, before_ram);
    assert_eq!(
        engine.guard_resident(KEY, old),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    let fresh = engine.compile_resident(1).unwrap().get();
    assert!(fresh > old);
    assert_eq!(engine.guard_resident(KEY, fresh), Ok(()));
    assert_eq!(
        engine.discard_unacknowledged_resident(KEY, old),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
}

use ring3_engine::{
    abi::x86::{decode_state, encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    process::{CallError, ResidentInstallation},
    windows::{CallingConvention32, WindowsApi32},
};

const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const OUTER: u32 = 0x3000;
const HOME: u32 = 0x4000;
const RETURN: u32 = HOME + 0x100;
const HOME_API: u32 = HOME + 0x200;
const ACTIVE: u32 = 0x6000;
const ACTIVE_API: u32 = ACTIVE + 0x100;
const STACK: u32 = 0x8f00;
const LAST_ERROR: u32 = 0xf123_4567;
const SIMPLE: [u8; 3] = [0x90, 0xeb, 0];
const PAGES: [u32; 6] = [CODE, KEEP, OUTER, HOME, ACTIVE, 0x8000];
const APIS: [WindowsApi32; 3] = [
    WindowsApi32::GetLastError,
    WindowsApi32::SetLastError,
    WindowsApi32::ExitProcess,
];

struct Fixture {
    engine: EngineInstance,
    ids: [u64; 5],
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    for (index, &(pc, length_or_id)) in blocks.iter().chain(gates).enumerate() {
        let at = 140 + index * 8;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&pc.to_le_bytes());
        engine.arena_mut().unwrap()[at + 4..at + 8].copy_from_slice(&length_or_id.to_le_bytes());
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
    id
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn stop(engine: &mut EngineInstance, pc: u32, reason: ExitReason) {
    // typed native stops prove ownership; the separate wasm fixture executes guest code.
    engine.write32(STACK, KEEP + 1).unwrap();
    engine.write32(STACK + 4, LAST_ERROR).unwrap();
    let state = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, STACK, 6, 7, 8],
        eip: pc,
        eflags: 0xcd7,
    };
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 1, reason },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

fn api_call(f: &mut Fixture, slot: usize, argument: u32) -> u32 {
    let api = APIS[slot];
    stop(
        &mut f.engine,
        KEEP + 0x100 + slot as u32 * 16,
        ExitReason::Gate { id: api.id() },
    );
    f.engine.write32(STACK + 4, argument).unwrap();
    let token = f
        .engine
        .capture_resident_call(KEY, f.ids[1], api.convention(), api.stack_words())
        .unwrap()
        .token;
    f.engine
        .complete_resident_windows_call(KEY, f.ids[1], token)
        .unwrap();
    decode_state(&f.engine.arena()[..56]).unwrap().registers[0]
}

fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    // every cap-test block is uploaded before any registry snapshot is published.
    for index in 0..=4 {
        upload(&mut engine, CODE + index * 16, &SIMPLE);
    }
    upload(&mut engine, KEEP, &SIMPLE);
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    upload(&mut engine, HOME, &[0x90]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    upload(&mut engine, HOME_API, &[0x0f, 0x0b]);
    upload(&mut engine, ACTIVE, &[0x90]);
    upload(&mut engine, ACTIVE_API, &[0x0f, 0x0b]);
    let mut keeper_blocks = vec![(KEEP, 3)];
    let mut keeper_gates = Vec::new();
    for (slot, api) in APIS.into_iter().enumerate() {
        let pc = KEEP + 0x100 + slot as u32 * 16;
        upload(&mut engine, pc, &[0x0f, 0x0b]);
        keeper_blocks.push((pc, 2));
        keeper_gates.push((pc, api.id()));
    }
    let ids = [
        compile(&mut engine, &[(CODE, 3)], &[]),
        compile(&mut engine, &keeper_blocks, &keeper_gates),
        compile(&mut engine, &[(OUTER, 2)], &[(OUTER, 17)]),
        compile(
            &mut engine,
            &[(HOME, 1), (RETURN, 2), (HOME_API, 2)],
            &[(RETURN, 18), (HOME_API, WindowsApi32::GetLastError.id())],
        ),
        compile(
            &mut engine,
            &[(ACTIVE, 1), (ACTIVE_API, 2)],
            &[(ACTIVE_API, WindowsApi32::GetLastError.id())],
        ),
    ];
    for (id, slot) in [(ids[1], 7), (ids[2], 2), (ids[3], 3), (ids[4], 4)] {
        engine
            .acknowledge_resident_installation(KEY, id, slot)
            .unwrap();
    }
    describe(
        &mut engine,
        &[(OUTER, 2), (HOME, 1), (RETURN, 2)],
        &[(OUTER, 17), (RETURN, 18)],
    );
    engine.compile_with_gates(3, 2).unwrap();
    engine.write32(0x8100, 0x89ab_cdef).unwrap();
    let mut f = Fixture { engine, ids };
    api_call(&mut f, 1, LAST_ERROR);
    assert_eq!(api_call(&mut f, 0, 0), LAST_ERROR);
    f
}

#[derive(Debug, PartialEq, Eq)]
struct UnitObservation {
    id: u64,
    bytes: Result<(Vec<u8>, usize), HostError>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    address: usize,
    key: u64,
    generation: u32,
    replacement: Result<(Vec<u8>, usize), HostError>,
    dispatcher: Result<(Vec<u8>, usize), HostError>,
    units: Vec<UnitObservation>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    memory: Result<(usize, u32, u32), HostError>,
}

fn observe(engine: &EngineInstance, ids: &[u64]) -> Observation {
    let owned = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Observation {
        arena: engine.arena().to_vec(),
        address: engine.arena_address(),
        key: engine.key(),
        generation: engine.generation(),
        replacement: engine.artifact_bytes().map(owned),
        dispatcher: engine.dispatcher_bytes(KEY).map(owned),
        units: ids
            .iter()
            .map(|&id| UnitObservation {
                id,
                bytes: engine.resident_bytes(id).map(owned),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
        installed: [KEEP, OUTER, HOME, RETURN, ACTIVE]
            .map(|pc| engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
        ram: PAGES
            .map(|pc| {
                let memory = engine.memory()?;
                let mut bytes = vec![0; 4096];
                memory
                    .read(GuestAddress(pc), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .to_vec(),
        memory: engine.memory().map(|memory| {
            (
                memory as *const _ as usize,
                memory.capacity_pages(),
                memory.mapped_pages(),
            )
        }),
    }
}

fn preserved(
    engine: &mut EngineInstance,
    ids: &[u64],
    operation: impl FnOnce(&mut EngineInstance),
) {
    let before = observe(engine, ids);
    let versions: Vec<_> = PAGES
        .iter()
        .filter_map(|&pc| {
            engine
                .memory()
                .ok()?
                .snapshot_code(GuestAddress(pc), 4096)
                .ok()
        })
        .collect();
    operation(engine);
    assert_eq!(observe(engine, ids), before);
    for version in versions {
        assert!(engine.memory().unwrap().is_code_current(&version));
    }
}

fn refuses(f: &mut Fixture, key: u64, id: u64, error: HostError) {
    preserved(&mut f.engine, &f.ids, |engine| {
        assert_eq!(engine.discard_unacknowledged_resident(key, id), Err(error));
    });
}

fn gone(engine: &mut EngineInstance, id: u64) {
    let error = Err(HostError::Resident(RegistryError::InvalidUnit));
    assert_eq!(engine.guard_resident(KEY, id), error);
    assert_eq!(engine.resident_bytes(id).map(|_| ()), error);
    assert_eq!(
        engine
            .acknowledge_resident_installation(KEY, id, 0)
            .map(|_| ()),
        error
    );
}

#[test]
fn unacknowledged_current_discard_recovers_real_capacity_without_mutating_keepers() {
    let mut f = fixture();
    let target = f.ids[0];
    for index in 1..=3 {
        compile(&mut f.engine, &[(CODE + index * 16, 3)], &[]);
    }
    f.engine.write8(ACTIVE, 0x90).unwrap();
    let stale = f.ids[4];
    refuses(
        &mut f,
        KEY,
        stale,
        HostError::Resident(RegistryError::CodeInvalidated),
    );
    preserved(&mut f.engine, &f.ids, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, target, 7),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(
            engine.retire_stale_resident(KEY, target),
            Err(HostError::Resident(RegistryError::CurrentUnit))
        );
    });
    describe(&mut f.engine, &[(CODE + 64, 3)], &[]);
    preserved(&mut f.engine, &f.ids, |engine| {
        assert_eq!(
            engine.compile_resident(1),
            Err(HostError::Resident(RegistryError::UnitCapacity))
        );
    });
    let kept = f.ids[1..].to_vec();
    preserved(&mut f.engine, &kept, |engine| {
        assert_eq!(engine.discard_unacknowledged_resident(KEY, target), Ok(()));
    });
    gone(&mut f.engine, target);
    let fresh = compile(&mut f.engine, &[(CODE, 3)], &[]);
    assert!(fresh > target);
    let mut observed = kept;
    observed.push(fresh);
    preserved(&mut f.engine, &observed, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, fresh, 0),
            Ok(ResidentInstallation {
                unit_id: fresh,
                slot: 0
            })
        );
        assert_eq!(
            engine.lookup_installed_resident(KEY, CODE),
            Ok(ResidentInstallation {
                unit_id: fresh,
                slot: 0
            })
        );
        assert_eq!(
            engine.discard_unacknowledged_resident(KEY, target),
            Err(HostError::Resident(RegistryError::InvalidUnit))
        );
    });
    describe(&mut f.engine, &[(CODE + 64, 3)], &[]);
    preserved(&mut f.engine, &observed, |engine| {
        assert_eq!(
            engine.compile_resident(1),
            Err(HostError::Resident(RegistryError::UnitCapacity))
        );
    });
    assert_eq!(api_call(&mut f, 0, 0), LAST_ERROR);
}

#[test]
fn full_identity_currency_installation_and_cancel_priorities_preserve_public_state() {
    let mut f = fixture();
    let foreign = fixture().ids[0];
    let target = f.ids[0];
    let keeper = f.ids[1];
    f.engine.write8(ACTIVE, 0x90).unwrap();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    f.engine.arena_mut().unwrap()[..96].fill(0xff);
    f.engine.arena_mut().unwrap()[140..252].fill(0xa5);
    for key in [0, KEY ^ 1, KEY ^ (1_u64 << 32)] {
        for id in [0, target, keeper, f.ids[4]] {
            refuses(&mut f, key, id, HostError::InvalidArtifact);
        }
    }
    // malformed high limbs do not claim a fabricated valid native large id.
    for id in [0, u64::MAX, target ^ (1_u64 << 32), foreign] {
        refuses(
            &mut f,
            KEY,
            id,
            HostError::Resident(RegistryError::InvalidUnit),
        );
    }
    let stale = f.ids[4];
    refuses(
        &mut f,
        KEY,
        stale,
        HostError::Resident(RegistryError::CodeInvalidated),
    );
    for installed in [keeper, f.ids[2], f.ids[3]] {
        refuses(&mut f, KEY, installed, HostError::InvalidRequest);
    }
    refuses(&mut f, KEY, target, HostError::Call(CallError::Cancelled));
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    preserved(&mut f.engine, &f.ids[1..], |engine| {
        assert_eq!(engine.discard_unacknowledged_resident(KEY, target), Ok(()));
    });
    gone(&mut f.engine, target);
    let fresh = compile(&mut f.engine, &[(CODE, 3)], &[]);
    f.engine
        .acknowledge_resident_installation(KEY, fresh, 0)
        .unwrap();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    preserved(&mut f.engine, &[fresh], |engine| {
        assert_eq!(
            engine.discard_unacknowledged_resident(KEY, fresh),
            Err(HostError::InvalidRequest)
        );
    });
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    assert_eq!(api_call(&mut f, 0, 0), LAST_ERROR);
}

fn outer_call(f: &mut Fixture, resident: bool) -> u32 {
    stop(&mut f.engine, OUTER, ExitReason::Gate { id: 17 });
    if resident {
        f.engine
            .capture_resident_call(KEY, f.ids[2], CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    } else {
        f.engine
            .capture_call(KEY, f.engine.generation(), CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    }
}

fn busy_matrix(f: &mut Fixture) {
    let target = f.ids[0];
    let ids = [0, target, f.ids[1], f.ids[2], f.ids[3], f.ids[4], u64::MAX];
    for id in ids {
        refuses(f, KEY, id, HostError::Call(CallError::Busy));
    }
    refuses(f, KEY ^ (1_u64 << 32), target, HostError::InvalidArtifact);
}

#[test]
fn any_pending_or_callback_blocks_discard_before_target_and_allows_original_cleanup() {
    for mode in [
        "replacement-pending",
        "resident-pending",
        "replacement-before-resume",
        "replacement-after-resume",
        "resident-unarmed",
        "resident-home",
        "resident-selected",
        "resident-selected-inner",
    ] {
        let mut f = fixture();
        let resident = mode.starts_with("resident");
        let outer = outer_call(&mut f, resident);
        let outer_records = f.engine.arena()[..96].to_vec();
        let mut callback = None;
        if !mode.ends_with("pending") {
            let token = if resident {
                f.engine
                    .begin_resident_callback(KEY, f.ids[2], f.ids[3], outer, HOME, RETURN, 18, &[])
                    .unwrap()
                    .token
            } else {
                f.engine
                    .begin_callback(KEY, f.engine.generation(), outer, HOME, RETURN, 18, &[])
                    .unwrap()
                    .token
            };
            callback = Some(token);
            if mode == "replacement-after-resume" {
                describe(
                    &mut f.engine,
                    &[(OUTER, 2), (HOME, 1), (RETURN, 2)],
                    &[(OUTER, 17), (RETURN, 18)],
                );
                f.engine
                    .resume_callback_code(KEY, f.engine.generation(), token, 3, 2)
                    .unwrap();
            }
            if matches!(
                mode,
                "resident-home" | "resident-selected" | "resident-selected-inner"
            ) {
                f.engine
                    .authorize_resident_callback(KEY, f.ids[3], token)
                    .unwrap();
            }
            if mode.starts_with("resident-selected") {
                stop(&mut f.engine, ACTIVE, ExitReason::NeedCode);
                f.engine
                    .select_resident_callback_unit(KEY, f.ids[3], token, f.ids[4])
                    .unwrap();
            }
            if mode == "resident-selected-inner" {
                stop(
                    &mut f.engine,
                    ACTIVE_API,
                    ExitReason::Gate {
                        id: WindowsApi32::GetLastError.id(),
                    },
                );
                f.engine
                    .capture_active_resident_callback_call(
                        KEY,
                        f.ids[4],
                        token,
                        CallingConvention32::Stdcall,
                        0,
                    )
                    .unwrap();
            }
        }
        f.engine.write8(KEEP, 0x90).unwrap();
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        f.engine.arena_mut().unwrap()[..96].fill(0xff);
        busy_matrix(&mut f);
        if let Some(token) = callback {
            f.engine.abort_callback(KEY, token).unwrap();
            assert_eq!(&f.engine.arena()[..96], outer_records.as_slice());
            busy_matrix(&mut f);
        }
        f.engine.abandon_call(KEY, outer).unwrap();
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        let kept = f.ids[1..].to_vec();
        let target = f.ids[0];
        preserved(&mut f.engine, &kept, |engine| {
            assert_eq!(engine.discard_unacknowledged_resident(KEY, target), Ok(()));
        });
        gone(&mut f.engine, target);
    }
}

#[test]
fn terminal_and_close_precede_identity_busy_currency_and_cancel() {
    for mode in ["exit-zero", "exit-max", "close-pending"] {
        let mut f = fixture();
        let error = if mode == "close-pending" {
            outer_call(&mut f, true);
            f.engine.close();
            HostError::Closed
        } else {
            api_call(&mut f, 2, if mode == "exit-max" { u32::MAX } else { 0 });
            HostError::ProcessExited
        };
        let before = observe(&f.engine, &f.ids);
        for key in [0, KEY, KEY ^ (1_u64 << 32)] {
            for id in [0, f.ids[0], f.ids[1], u64::MAX] {
                assert_eq!(
                    f.engine.discard_unacknowledged_resident(key, id),
                    Err(error)
                );
                assert_eq!(observe(&f.engine, &f.ids), before);
            }
        }
        f.engine.close();
        f.engine.close();
        assert_eq!(f.engine.arena(), before.arena);
        assert_eq!(f.engine.arena_address(), before.address);
        assert_eq!(f.engine.key(), KEY);
        assert_eq!(f.engine.generation(), 0);
        assert_eq!(
            f.engine.discard_unacknowledged_resident(0, 0),
            Err(HostError::Closed)
        );
    }
}
