use ring3_engine::{
    abi::x86::{decode_state, encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0x1020_3040_5060_7080;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const OUTER: u32 = 0x3000;
const HOME: u32 = 0x4000;
const RETURN: u32 = HOME + 0x100;
const STACK: u32 = 0x8f00;
const LAST_ERROR: u32 = 0xf123_4567;
const PAGES: [u32; 6] = [CODE, KEEP, OUTER, HOME, 0x6000, 0x8000];
const SIMPLE: [u8; 3] = [0x90, 0xeb, 0];
const APIS: [WindowsApi32; 3] = [
    WindowsApi32::GetLastError,
    WindowsApi32::SetLastError,
    WindowsApi32::ExitProcess,
];

struct Fixture {
    engine: EngineInstance,
    ids: [u64; 4],
}

fn describe(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    for (index, &(pc, value)) in blocks.iter().chain(gates).enumerate() {
        let at = 140 + index * 8;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&pc.to_le_bytes());
        engine.arena_mut().unwrap()[at + 4..at + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    describe(engine, blocks, gates);
    engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get()
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn stop(engine: &mut EngineInstance, pc: u32, gate: u32) {
    // typed native stops exercise ownership; actual wasm owns guest execution proof.
    let state = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, STACK, 6, 7, 8],
        eip: pc,
        eflags: 0xcd7,
    };
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: gate },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.write32(STACK, KEEP + 1).unwrap();
    engine.write32(STACK + 4, LAST_ERROR).unwrap();
}

fn api_call(f: &mut Fixture, slot: usize) -> u32 {
    let api = APIS[slot];
    stop(&mut f.engine, KEEP + 0x100 + slot as u32 * 16, api.id());
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

fn fixture(installed: bool) -> Fixture {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    for index in 0..=4 {
        upload(&mut engine, CODE + index * 16, &SIMPLE);
    }
    upload(&mut engine, KEEP, &SIMPLE);
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    upload(&mut engine, HOME, &[0x90]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
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
        compile(&mut engine, &[(HOME, 1), (RETURN, 2)], &[(RETURN, 18)]),
    ];
    for (id, slot) in [(ids[1], 7), (ids[2], 2), (ids[3], 3)] {
        engine
            .acknowledge_resident_installation(KEY, id, slot)
            .unwrap();
    }
    if installed {
        engine
            .acknowledge_resident_installation(KEY, ids[0], 0)
            .unwrap();
    }
    describe(
        &mut engine,
        &[(OUTER, 2), (HOME, 1), (RETURN, 2)],
        &[(OUTER, 17), (RETURN, 18)],
    );
    engine.compile_with_gates(3, 2).unwrap();
    engine.write32(0x6100, 0x89ab_cdef).unwrap();
    let mut f = Fixture { engine, ids };
    api_call(&mut f, 1);
    assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    f
}

#[derive(Debug, PartialEq, Eq)]
struct UnitObservation {
    id: u64,
    module: Result<(Vec<u8>, usize), HostError>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    replacement: Result<(Vec<u8>, usize), HostError>,
    dispatcher: Result<(Vec<u8>, usize), HostError>,
    units: Vec<UnitObservation>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    mapped: Result<u32, HostError>,
}

fn observe(engine: &EngineInstance, ids: &[u64]) -> Observation {
    let retained = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Observation {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        generation: engine.generation(),
        replacement: engine.artifact_bytes().map(retained),
        dispatcher: engine.dispatcher_bytes(KEY).map(retained),
        units: ids
            .iter()
            .map(|&id| UnitObservation {
                id,
                module: engine.resident_bytes(id).map(retained),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
        installed: [KEEP, OUTER, HOME, RETURN]
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
        mapped: engine.memory().map(|memory| memory.mapped_pages()),
    }
}

fn preserved(
    engine: &mut EngineInstance,
    ids: &[u64],
    operation: impl FnOnce(&mut EngineInstance),
) {
    let before = observe(engine, ids);
    let versions = PAGES.map(|pc| {
        engine
            .memory()
            .ok()
            .and_then(|memory| memory.snapshot_code(GuestAddress(pc), 4096).ok())
    });
    operation(engine);
    assert_eq!(observe(engine, ids), before);
    for snapshot in versions.iter().flatten() {
        assert!(engine.memory().unwrap().is_code_current(snapshot));
    }
}

fn refuses(f: &mut Fixture, key: u64, id: u64, error: HostError) {
    preserved(&mut f.engine, &f.ids, |engine| {
        assert_eq!(engine.retire_stale_resident(key, id), Err(error));
    });
}

fn retired(engine: &EngineInstance, id: u64) {
    let error = Err(HostError::Resident(RegistryError::InvalidUnit));
    assert_eq!(engine.guard_resident(KEY, id), error);
    assert_eq!(engine.resident_bytes(id).map(|_| ()), error);
}

#[test]
fn stale_unit_requires_an_explicit_retirement_path_to_reclaim_capacity() {
    let mut f = fixture(true);
    for index in 1..=4 {
        compile(&mut f.engine, &[(CODE + index * 16, 3)], &[]);
    }
    f.engine.write8(CODE, 0x90).unwrap();
    describe(&mut f.engine, &[(CODE, 3)], &[]);
    preserved(&mut f.engine, &f.ids, |engine| {
        assert_eq!(
            engine.compile_resident(1).map(|_| ()),
            Err(HostError::Resident(RegistryError::UnitCapacity))
        );
    });
    let mut previous = f.ids[0];
    let mut old_ids = Vec::new();
    for _ in 0..9 {
        preserved(&mut f.engine, &f.ids[1..], |engine| {
            assert_eq!(engine.retire_stale_resident(KEY, previous), Ok(()));
        });
        old_ids.push(previous);
        let fresh = compile(&mut f.engine, &[(CODE, 3)], &[]);
        assert!(fresh > previous && !old_ids.contains(&fresh));
        f.engine
            .acknowledge_resident_installation(KEY, fresh, 0)
            .unwrap();
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, CODE),
            Ok(ResidentInstallation {
                unit_id: fresh,
                slot: 0
            })
        );
        for &id in &old_ids {
            retired(&f.engine, id);
        }
        describe(&mut f.engine, &[(CODE + 64, 3)], &[]);
        assert_eq!(
            f.engine.compile_resident(1).map(|_| ()),
            Err(HostError::Resident(RegistryError::UnitCapacity))
        );
        f.engine.write8(CODE, 0x90).unwrap();
        previous = fresh;
    }
    assert_eq!(api_call(&mut f, 0), LAST_ERROR);
}

#[test]
fn installed_and_uninstalled_retirement_preserves_other_owners_and_frees_only_matching_slot() {
    for installed in [false, true] {
        let mut f = fixture(installed);
        f.engine.write8(CODE, 0x90).unwrap();
        let old = f.ids[0];
        preserved(&mut f.engine, &f.ids[1..], |engine| {
            assert_eq!(engine.retire_stale_resident(KEY, old), Ok(()));
        });
        retired(&f.engine, old);
        let fresh = compile(&mut f.engine, &[(CODE, 3)], &[]);
        f.engine
            .acknowledge_resident_installation(KEY, fresh, 0)
            .unwrap();
        preserved(&mut f.engine, &[fresh], |engine| {
            assert_eq!(
                engine.acknowledge_resident_installation(KEY, fresh, 7),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(
                engine.retire_stale_resident(KEY, old),
                Err(HostError::Resident(RegistryError::InvalidUnit))
            );
        });
        retired(&f.engine, old);
        assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    }
}

#[test]
fn all_snapshot_invalidation_shapes_are_eligible_without_memory_repair() {
    for mutation in ["same-byte", "protect", "unmap", "remap"] {
        let mut f = fixture(true);
        match mutation {
            "same-byte" => f.engine.write8(CODE, 0x90).unwrap(),
            "protect" => f.engine.protect(CODE, 1, 3).unwrap(),
            "unmap" => f.engine.unmap(CODE, 1).unwrap(),
            "remap" => {
                f.engine.unmap(CODE, 1).unwrap();
                f.engine.map(CODE, 1, 7).unwrap();
            }
            _ => unreachable!(),
        }
        let old = f.ids[0];
        assert_eq!(
            f.engine.guard_resident(KEY, old),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        preserved(&mut f.engine, &f.ids[1..], |engine| {
            assert_eq!(engine.retire_stale_resident(KEY, old), Ok(()));
        });
        retired(&f.engine, old);
        assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    }
}

#[test]
fn full_identity_and_eligibility_precede_cancellation_without_reading_public_state() {
    let mut f = fixture(true);
    f.engine.write8(CODE, 0x90).unwrap();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    f.engine.arena_mut().unwrap()[..96].fill(0xff);
    let old = f.ids[0];
    let keeper = f.ids[1];
    for key in [0, KEY ^ 1, KEY ^ (1_u64 << 32)] {
        for id in [0, old, keeper] {
            refuses(&mut f, key, id, HostError::InvalidArtifact);
        }
    }
    for id in [0, old ^ (1_u64 << 32), u64::MAX] {
        refuses(
            &mut f,
            KEY,
            id,
            HostError::Resident(RegistryError::InvalidUnit),
        );
    }
    refuses(
        &mut f,
        KEY,
        keeper,
        HostError::Resident(RegistryError::CurrentUnit),
    );
    refuses(&mut f, KEY, old, HostError::Call(CallError::Cancelled));
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    let fresh = compile(&mut f.engine, &[(CODE, 3)], &[]);
    let mut observed = f.ids.to_vec();
    observed.push(fresh);
    preserved(&mut f.engine, &observed, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, fresh, 0),
            Err(HostError::InvalidRequest)
        );
    });
    let kept = [fresh, f.ids[1], f.ids[2], f.ids[3]];
    preserved(&mut f.engine, &kept, |engine| {
        assert_eq!(engine.retire_stale_resident(KEY, old), Ok(()));
    });
    let installed = ResidentInstallation {
        unit_id: fresh,
        slot: 0,
    };
    preserved(&mut f.engine, &kept, |engine| {
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, fresh, 0),
            Ok(installed)
        );
    });
    assert_eq!(f.engine.lookup_installed_resident(KEY, CODE), Ok(installed));
    retired(&f.engine, old);
    refuses(
        &mut f,
        KEY,
        old,
        HostError::Resident(RegistryError::InvalidUnit),
    );
    assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    preserved(&mut empty, &[], |engine| {
        assert_eq!(
            engine.retire_stale_resident(KEY, 0),
            Err(HostError::Resident(RegistryError::InvalidUnit))
        );
    });
}

fn outer_call(f: &mut Fixture, resident: bool) -> u32 {
    stop(&mut f.engine, OUTER, 17);
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
    let stale = f.ids[0];
    for id in [0, stale, f.ids[1], f.ids[2], f.ids[3], u64::MAX] {
        refuses(f, KEY, id, HostError::Call(CallError::Busy));
    }
    refuses(f, KEY ^ (1_u64 << 32), stale, HostError::InvalidArtifact);
}

#[test]
fn replacement_and_resident_pending_calls_block_retirement_even_when_the_owner_is_stale() {
    for resident in [false, true] {
        let mut f = fixture(true);
        let token = outer_call(&mut f, resident);
        f.engine.write8(CODE, 0x90).unwrap();
        f.engine.write8(OUTER, 0x0f).unwrap();
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        busy_matrix(&mut f);
        f.engine.abandon_call(KEY, token).unwrap();
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        for index in [0, 2] {
            f.engine.retire_stale_resident(KEY, f.ids[index]).unwrap();
            retired(&f.engine, f.ids[index]);
        }
        assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    }
}

#[test]
fn callbacks_block_retirement_before_eligibility_with_stale_home_and_outer_owners() {
    for (resident, armed) in [(false, false), (true, false), (true, true)] {
        let mut f = fixture(true);
        let outer = outer_call(&mut f, resident);
        let callback = if resident {
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
        if armed {
            f.engine
                .authorize_resident_callback(KEY, f.ids[3], callback)
                .unwrap();
        }
        for (pc, value) in [(CODE, 0x90), (OUTER, 0x0f), (HOME, 0x90)] {
            f.engine.write8(pc, value).unwrap();
        }
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        busy_matrix(&mut f);
        f.engine.abort_callback(KEY, callback).unwrap();
        busy_matrix(&mut f);
        f.engine.abandon_call(KEY, outer).unwrap();
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        for index in [0, 2, 3] {
            f.engine.retire_stale_resident(KEY, f.ids[index]).unwrap();
            retired(&f.engine, f.ids[index]);
        }
        assert_eq!(api_call(&mut f, 0), LAST_ERROR);
    }
}

#[test]
fn process_exit_and_close_precede_wrong_key_busy_and_eligibility() {
    for close in [false, true] {
        let mut f = fixture(true);
        f.engine.write8(CODE, 0x90).unwrap();
        if close {
            outer_call(&mut f, true);
            f.engine.close();
        } else {
            api_call(&mut f, 2);
        }
        let error = if close {
            HostError::Closed
        } else {
            HostError::ProcessExited
        };
        let ids = [0, f.ids[0], f.ids[1], u64::MAX];
        for key in [KEY, 0, KEY ^ (1_u64 << 32)] {
            for id in ids {
                refuses(&mut f, key, id, error);
            }
        }
    }
}
