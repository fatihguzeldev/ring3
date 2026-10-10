#![forbid(unsafe_code)]

use super::{EngineInstance, HostError, ResidentInstallation};
use crate::{
    abi::{
        arena::{
            ARENA_SIZE, EXIT_OFFSET, HELPER_OFFSET, TRANSFER_OFFSET, TRANSFER_SIZE, X87_OFFSET,
        },
        memory_helper::{HELPER_SIZE, encode_helper_result},
        x86::{
            EXIT_SIZE, X87_SIZE, decode_x87, encode_exit, encode_exit_v3, encode_state, encode_x87,
        },
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{RegistryError, RegistryUsage},
        x86::{State32, X87State},
    },
    memory::{Access, BackingOffset, GuestAddress, MemoryError, PageRange},
    windows::{CallFrame32, CallingConvention32, ProcessContext32, WindowsApi32, WindowsOutcome32},
};

const KEY: u64 = 0x8172_6354_4536_2718;
const CODE: u32 = 0x1000;
const REPLACEMENT: u32 = 0x2000;
const SET_ERROR: u32 = 0x2100;
const STACK: u32 = 0x8f00;
const LAST_ERROR: u32 = 0xf123_4567;
const PAGES: [u32; 3] = [CODE, REPLACEMENT, 0x8000];
const SIMPLE: [u8; 3] = [0x90, 0xeb, 0];

fn describe(engine: &mut EngineInstance, pc: u32, entries: bool) {
    let transfer =
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
    transfer[..4].copy_from_slice(&pc.to_le_bytes());
    if !entries {
        transfer[4..8].copy_from_slice(&(SIMPLE.len() as u32).to_le_bytes());
    }
}

fn compile(engine: &mut EngineInstance, pc: u32, entries: bool) -> u64 {
    describe(engine, pc, entries);
    if entries {
        engine.compile_resident_entries(1, 0)
    } else {
        engine.compile_resident(1)
    }
    .unwrap()
    .get()
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn seed(engine: &mut EngineInstance) -> X87State {
    let state = State32 {
        registers: [
            0x1122_3344,
            0x2233_4455,
            0x3344_5566,
            0x4455_6677,
            STACK,
            0x6677_8899,
            0x7788_99aa,
            0x8899_aabb,
        ],
        eip: CODE,
        eflags: 0xcd7,
    };
    let fp = X87State {
        status: 0x2000,
        tag: 0xfffc,
        opcode: 0x123,
        instruction_pointer: CODE + 1,
        data_pointer: 0x8100,
        code_selector: 0x23,
        data_selector: 0x2b,
        registers: std::array::from_fn(|index| {
            std::array::from_fn(|byte| (index * 19 + byte * 7 + 1) as u8)
        }),
        ..X87State::default()
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Budget,
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    arena[96..100].copy_from_slice(&0_u32.to_le_bytes());
    encode_helper_result(
        Ok(0x89ab_cdef),
        &mut arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )
    .unwrap();
    for (index, byte) in arena[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE]
        .iter_mut()
        .enumerate()
    {
        *byte = (index.wrapping_mul(29).wrapping_add(7)) as u8;
    }
    encode_x87(&fp, &mut arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    fp
}

struct Fixture {
    engine: EngineInstance,
    ids: [u64; 8],
    probe: CallFrame32,
    fp: X87State,
}

fn fixture(entries: bool) -> Fixture {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    // preload every region before any retained code snapshot is compiled.
    for index in 0..9 {
        upload(&mut engine, CODE + index * 16, &SIMPLE);
    }
    upload(&mut engine, REPLACEMENT, &SIMPLE);
    upload(&mut engine, SET_ERROR, &[0x0f, 0x0b]);
    engine.protect(CODE, 1, 5).unwrap();
    engine.protect(REPLACEMENT, 1, 5).unwrap();
    engine.protect(0x8000, 1, 3).unwrap();
    let ids = std::array::from_fn(|slot| {
        let id = compile(&mut engine, CODE + slot as u32 * 16, entries);
        assert_eq!(
            engine.acknowledge_resident_installation(KEY, id, slot as u32),
            Ok(ResidentInstallation {
                unit_id: id,
                slot: slot as u32,
            })
        );
        id
    });
    for (index, word) in [
        REPLACEMENT,
        SIMPLE.len() as u32,
        SET_ERROR,
        2,
        SET_ERROR,
        WindowsApi32::SetLastError.id(),
    ]
    .into_iter()
    .enumerate()
    {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(2, 1).unwrap();
    engine.write32(STACK, REPLACEMENT + 1).unwrap();
    engine.write32(STACK + 4, LAST_ERROR).unwrap();
    let state = State32 {
        registers: [10, 2, 3, 4, STACK, 6, 7, 8],
        eip: SET_ERROR,
        eflags: 0xcd7,
    };
    let probe = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    // a typed stop populates windows ownership; it is not guest execution evidence.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate {
                id: WindowsApi32::SetLastError.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(KEY, generation, CallingConvention32::Stdcall, 1)
        .unwrap()
        .token;
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    let fp = seed(&mut engine);
    Fixture {
        engine,
        ids,
        probe,
        fp,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct PageObservation {
    address: u32,
    bytes: Vec<u8>,
    access: [Result<BackingOffset, MemoryError>; 3],
    code: Option<(u64, usize, Vec<(u64, u64)>)>,
}

#[derive(Debug, PartialEq, Eq)]
struct Retained {
    arena: Vec<u8>,
    arena_address: usize,
    key: u64,
    generation: u32,
    call_token: u32,
    exit_code: Option<u32>,
    pending: bool,
    callback: bool,
    image: Option<crate::loader::ImageMetadata32>,
    image_input: bool,
    image_started: bool,
    last_error: u32,
    virtual_allocations: Vec<PageRange>,
    memory: (u64, u32, u32),
    replacement: (Vec<u8>, usize),
    dispatcher: (Vec<u8>, usize),
    usage: RegistryUsage,
    units: Vec<(u64, Vec<u8>, usize)>,
    installations: [Option<ResidentInstallation>; 8],
    pages: Vec<PageObservation>,
}

fn retained(f: &Fixture) -> Retained {
    let engine = &f.engine;
    let memory = engine.memory.as_ref().unwrap();
    let owned = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    let (outcome, _) = engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &f.probe,
            ProcessContext32::default(),
        )
        .unwrap();
    let WindowsOutcome32::Return(last_error) = outcome else {
        unreachable!()
    };
    Retained {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        key: engine.key,
        generation: engine.generation,
        call_token: engine.call_token,
        exit_code: engine.exit_code,
        pending: engine.pending_call.is_some(),
        callback: engine.callback.is_some(),
        image: engine.image,
        image_input: engine.image_input.is_some(),
        image_started: engine.image_started,
        last_error,
        virtual_allocations: engine.virtual_allocations.clone(),
        memory: (
            memory.identity(),
            memory.capacity_pages(),
            memory.mapped_pages(),
        ),
        replacement: owned(
            engine
                .artifact
                .as_ref()
                .unwrap()
                .wasm_bytes(memory)
                .unwrap(),
        ),
        dispatcher: owned(engine.dispatcher.as_ref().unwrap()),
        usage: engine.resident.as_ref().unwrap().usage(),
        units: f
            .ids
            .iter()
            .map(|&id| {
                let bytes = engine
                    .resident
                    .as_ref()
                    .unwrap()
                    .get_raw(memory, id)
                    .unwrap()
                    .wasm_bytes(memory)
                    .unwrap();
                (id, bytes.to_vec(), bytes.as_ptr() as usize)
            })
            .collect(),
        installations: engine.resident_installations,
        pages: PAGES
            .into_iter()
            .map(|address| {
                let mut bytes = vec![0; 4096];
                memory.read(GuestAddress(address), &mut bytes).unwrap();
                PageObservation {
                    address,
                    bytes,
                    access: [Access::Read, Access::Write, Access::Execute]
                        .map(|access| memory.resolve(GuestAddress(address), access)),
                    code: memory
                        .snapshot_code(GuestAddress(address), 4096)
                        .ok()
                        .map(|snapshot| {
                            (
                                snapshot.identity,
                                snapshot.first,
                                snapshot
                                    .versions
                                    .iter()
                                    .map(|version| (version.mapping, version.content))
                                    .collect(),
                            )
                        }),
                }
            })
            .collect(),
    }
}

#[test]
fn existing_public_apis_cannot_reclaim_eight_installed_current_units() {
    for entries in [false, true] {
        let mut f = fixture(entries);
        describe(&mut f.engine, CODE + 8 * 16, entries);
        let before = retained(&f);
        assert_eq!(before.arena.len(), ARENA_SIZE);
        assert_eq!(before.usage.units, 8);
        assert_eq!(
            before.usage.wasm_bytes,
            before.units.iter().map(|unit| unit.1.len()).sum::<usize>()
        );
        assert_eq!(before.last_error, LAST_ERROR);
        assert_eq!(before.call_token, 1);
        assert_eq!(
            decode_x87(&before.arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap(),
            f.fp
        );
        assert!(
            !before.pending
                && !before.callback
                && before.image.is_none()
                && !before.image_input
                && !before.image_started
                && before.exit_code.is_none()
                && before.virtual_allocations.is_empty()
        );
        let ninth = if entries {
            f.engine.compile_resident_entries(1, 0)
        } else {
            f.engine.compile_resident(1)
        };
        assert_eq!(ninth, Err(HostError::Resident(RegistryError::UnitCapacity)));
        assert_eq!(retained(&f), before);
        for (slot, id) in f.ids.into_iter().enumerate() {
            assert_eq!(f.engine.guard_resident(KEY, id), Ok(()));
            assert_eq!(
                f.engine
                    .lookup_installed_resident(KEY, CODE + slot as u32 * 16),
                Ok(ResidentInstallation {
                    unit_id: id,
                    slot: slot as u32,
                })
            );
            assert_eq!(
                f.engine.discard_unacknowledged_resident(KEY, id),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(retained(&f), before);
            assert_eq!(
                f.engine.retire_stale_resident(KEY, id),
                Err(HostError::Resident(RegistryError::CurrentUnit))
            );
            assert_eq!(retained(&f), before);
        }
    }
}

use super::{
    CallError,
    call::{PendingCall, PendingOwner},
    callback::SuspendedRecord,
};
use crate::{
    abi::{callback::CallbackRecord32, resident_callback::ResidentCallbackRecord32},
    cpu::dbt::{BlockSpec, CompileLimits, RegistryLimits, ResidentRegistry, compile_region},
};

#[derive(Debug, PartialEq, Eq)]
struct PendingObservation {
    token: u32,
    owner: PendingOwner,
    frame: CallFrame32,
    state: [u8; 56],
    exit: [u8; EXIT_SIZE],
    x87: [u8; X87_SIZE],
}

fn observe_pending(pending: &PendingCall) -> PendingObservation {
    PendingObservation {
        token: pending.token,
        owner: pending.owner,
        frame: pending.frame,
        state: pending.state,
        exit: pending.exit,
        x87: pending.x87,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CallbackObservation {
    Replacement(CallbackRecord32, PendingObservation),
    Resident {
        record: ResidentCallbackRecord32,
        authorized: bool,
        active_unit_id: u64,
        outer: PendingObservation,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct StableOwners {
    arena: Vec<u8>,
    arena_address: usize,
    key: u64,
    generation: u32,
    call_token: u32,
    exit_code: Option<u32>,
    image: Option<crate::loader::ImageMetadata32>,
    image_input_address: Option<usize>,
    image_started: bool,
    pending: Option<PendingObservation>,
    callback: Option<CallbackObservation>,
    last_error: u32,
    virtual_allocations: Vec<PageRange>,
    memory: Option<(u64, u32, u32)>,
    replacement: Option<(String, Option<(Vec<u8>, usize)>)>,
    dispatcher: Option<(Vec<u8>, usize)>,
    pages: Vec<PageObservation>,
}

type ModuleObservation = (u64, Result<(Vec<u8>, usize), HostError>);

#[derive(Debug, PartialEq, Eq)]
struct OwnershipObservation {
    stable: StableOwners,
    usage: Option<RegistryUsage>,
    installations: [Option<ResidentInstallation>; 8],
    units: Vec<ModuleObservation>,
    registry: Option<String>,
}

fn observe(
    engine: &EngineInstance,
    ids: &[u64],
    probe: &CallFrame32,
    pages: &[u32],
) -> OwnershipObservation {
    let owned = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    let (outcome, _) = engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            probe,
            ProcessContext32::default(),
        )
        .unwrap();
    let WindowsOutcome32::Return(last_error) = outcome else {
        unreachable!()
    };
    let callback = engine
        .callback
        .as_ref()
        .map(|callback| match callback.record {
            SuspendedRecord::Replacement(record) => {
                CallbackObservation::Replacement(record, observe_pending(&callback.outer))
            }
            SuspendedRecord::Resident {
                record,
                authorized,
                active_unit_id,
            } => CallbackObservation::Resident {
                record,
                authorized,
                active_unit_id,
                outer: observe_pending(&callback.outer),
            },
        });
    let memory = engine.memory.as_ref();
    OwnershipObservation {
        stable: StableOwners {
            arena: engine.arena().to_vec(),
            arena_address: engine.arena_address(),
            key: engine.key,
            generation: engine.generation,
            call_token: engine.call_token,
            exit_code: engine.exit_code,
            image: engine.image,
            // the input's private byte vector is deliberately not exposed to this sibling module.
            image_input_address: engine
                .image_input
                .as_ref()
                .map(|input| input as *const _ as usize),
            image_started: engine.image_started,
            pending: engine.pending_call.as_ref().map(observe_pending),
            callback,
            last_error,
            virtual_allocations: engine.virtual_allocations.clone(),
            memory: memory.map(|memory| {
                (
                    memory.identity(),
                    memory.capacity_pages(),
                    memory.mapped_pages(),
                )
            }),
            replacement: engine.artifact.as_ref().map(|artifact| {
                let bytes = memory
                    .and_then(|memory| artifact.wasm_bytes(memory).ok())
                    .map(owned);
                (format!("{artifact:?}"), bytes)
            }),
            dispatcher: engine.dispatcher.as_deref().map(owned),
            pages: memory.map_or_else(Vec::new, |memory| {
                pages
                    .iter()
                    .map(|&address| {
                        let mut bytes = vec![0; 4096];
                        memory.read(GuestAddress(address), &mut bytes).unwrap();
                        PageObservation {
                            address,
                            bytes,
                            access: [Access::Read, Access::Write, Access::Execute]
                                .map(|access| memory.resolve(GuestAddress(address), access)),
                            code: memory.snapshot_code(GuestAddress(address), 4096).ok().map(
                                |snapshot| {
                                    (
                                        snapshot.identity,
                                        snapshot.first,
                                        snapshot
                                            .versions
                                            .iter()
                                            .map(|version| (version.mapping, version.content))
                                            .collect(),
                                    )
                                },
                            ),
                        }
                    })
                    .collect()
            }),
        },
        usage: engine.resident.as_ref().map(ResidentRegistry::usage),
        installations: engine.resident_installations,
        units: ids
            .iter()
            .map(|&id| {
                let result = match (memory, engine.resident.as_ref()) {
                    (None, _) => Err(HostError::Closed),
                    (Some(memory), Some(registry)) => registry
                        .get_raw(memory, id)
                        .map_err(HostError::Resident)
                        .and_then(|unit| {
                            unit.wasm_bytes(memory)
                                .map(owned)
                                .map_err(|_| HostError::Resident(RegistryError::CodeInvalidated))
                        }),
                    (Some(_), None) => Err(HostError::Resident(RegistryError::InvalidUnit)),
                };
                (id, result)
            })
            .collect(),
        // debug includes immutable retained bytes and prepared code snapshots, even for stale units.
        registry: engine
            .resident
            .as_ref()
            .map(|registry| format!("{registry:?}")),
    }
}

fn refuse(
    engine: &mut EngineInstance,
    ids: &[u64],
    probe: &CallFrame32,
    pages: &[u32],
    request: (u64, u64, u32),
    expected: HostError,
) {
    let before = observe(engine, ids, probe, pages);
    assert_eq!(
        engine.discard_installed_resident(request.0, request.1, request.2),
        Err(expected)
    );
    assert_eq!(observe(engine, ids, probe, pages), before);
}

fn remove_exact(
    engine: &mut EngineInstance,
    ids: &[u64],
    probe: &CallFrame32,
    pages: &[u32],
    id: u64,
    slot: u32,
) -> usize {
    let before = observe(engine, ids, probe, pages);
    assert_eq!(engine.memory().map(|_| ()), Ok(()));
    assert_eq!(engine.key, KEY);
    assert!(engine.pending_call.is_none() && engine.callback.is_none());
    assert_eq!(&engine.arena()[96..100], &[0; 4]);
    assert_eq!(engine.arena().len(), ARENA_SIZE);
    decode_x87(&engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
    let length = before
        .units
        .iter()
        .find(|unit| unit.0 == id)
        .unwrap()
        .1
        .as_ref()
        .unwrap()
        .0
        .len();
    assert_eq!(engine.discard_installed_resident(KEY, id, slot), Ok(()));
    let after = observe(engine, ids, probe, pages);
    assert_eq!(after.stable, before.stable);
    assert_eq!(
        after.usage,
        before.usage.map(|usage| RegistryUsage {
            units: usage.units - 1,
            wasm_bytes: usage.wasm_bytes - length,
        })
    );
    let mut installations = before.installations;
    assert_eq!(
        installations[slot as usize],
        Some(ResidentInstallation { unit_id: id, slot })
    );
    installations[slot as usize] = None;
    assert_eq!(after.installations, installations);
    let expected: Vec<_> = before
        .units
        .into_iter()
        .map(|(unit_id, result)| {
            (
                unit_id,
                if unit_id == id {
                    Err(HostError::Resident(RegistryError::InvalidUnit))
                } else {
                    result
                },
            )
        })
        .collect();
    assert_eq!(after.units, expected);
    assert_eq!(
        engine.guard_resident(KEY, id),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    assert_eq!(
        engine.resident_bytes(id),
        Err(HostError::Resident(RegistryError::InvalidUnit))
    );
    length
}

#[test]
fn discard_installed_reclaims_first_middle_last_and_reuses_slots_without_id_aba() {
    for entries in [false, true] {
        for slot in [0, 3, 7] {
            let mut f = fixture(entries);
            let mut all = f.ids.to_vec();
            let original = f.ids[slot];
            let mut removed = Vec::new();
            for cycle in 0..4 {
                let current = f.ids[slot];
                let old_usage = f.engine.resident.as_ref().unwrap().usage();
                remove_exact(&mut f.engine, &all, &f.probe, &PAGES, current, slot as u32);
                removed.push(current);
                let pc = if cycle % 2 == 0 {
                    CODE + 128
                } else {
                    CODE + slot as u32 * 16
                };
                let next = compile(&mut f.engine, pc, entries);
                assert!(next > *all.iter().max().unwrap());
                all.push(next);
                f.ids[slot] = next;
                f.engine
                    .acknowledge_resident_installation(KEY, next, slot as u32)
                    .unwrap();
                assert_eq!(
                    f.engine.resident.as_ref().unwrap().usage().units,
                    old_usage.units
                );
                assert_eq!(
                    f.engine.lookup_installed_resident(KEY, pc),
                    Ok(ResidentInstallation {
                        unit_id: next,
                        slot: slot as u32
                    })
                );
                for &id in &removed {
                    refuse(
                        &mut f.engine,
                        &all,
                        &f.probe,
                        &PAGES,
                        (KEY, id, slot as u32),
                        HostError::Resident(RegistryError::InvalidUnit),
                    );
                }
                assert_eq!(
                    f.engine.guard_resident(KEY, original),
                    Err(HostError::Resident(RegistryError::InvalidUnit))
                );
            }
        }
    }
}

#[test]
fn discard_installed_returns_exact_byte_credit_under_a_tight_registry_budget() {
    let mut f = fixture(false);
    let memory = f.engine.memory.as_ref().unwrap();
    let spec = |pc| BlockSpec {
        entry: GuestAddress(pc),
        byte_length: 3,
    };
    let first = compile_region(memory, &[spec(CODE)], CompileLimits::default()).unwrap();
    let second = compile_region(memory, &[spec(CODE + 16)], CompileLimits::default()).unwrap();
    let first_length = first.wasm_bytes(memory).unwrap().len();
    let second_length = second.wasm_bytes(memory).unwrap().len();
    let mut registry = ResidentRegistry::new(
        memory,
        RegistryLimits {
            units: 8,
            wasm_bytes: first_length + second_length - 1,
        },
    )
    .unwrap();
    let id = registry
        .compile(memory, &[spec(CODE)], CompileLimits::default())
        .unwrap()
        .get();
    f.engine.resident = Some(registry);
    f.engine.resident_installations = [None; 8];
    f.engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    let before = observe(&f.engine, &[id], &f.probe, &PAGES);
    assert_eq!(
        f.engine.resident.as_mut().unwrap().compile(
            f.engine.memory.as_ref().unwrap(),
            &[spec(CODE + 16)],
            CompileLimits::default()
        ),
        Err(RegistryError::ByteCapacity)
    );
    assert_eq!(observe(&f.engine, &[id], &f.probe, &PAGES), before);
    assert_eq!(
        remove_exact(&mut f.engine, &[id], &f.probe, &PAGES, id, 0),
        first_length
    );
    assert_eq!(
        f.engine.resident.as_ref().unwrap().usage(),
        RegistryUsage {
            units: 0,
            wasm_bytes: 0
        }
    );
    let next = f
        .engine
        .resident
        .as_mut()
        .unwrap()
        .compile(
            f.engine.memory.as_ref().unwrap(),
            &[spec(CODE + 16)],
            CompileLimits::default(),
        )
        .unwrap()
        .get();
    assert!(next > id);
    f.engine
        .acknowledge_resident_installation(KEY, next, 0)
        .unwrap();
    assert_eq!(
        f.engine.resident.as_ref().unwrap().usage(),
        RegistryUsage {
            units: 1,
            wasm_bytes: second_length
        }
    );
}

#[test]
fn discard_installed_orders_key_identity_currency_exact_slot_and_cancellation() {
    let mut f = fixture(false);
    let id = f.ids[3];
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    for key in [0, KEY ^ 1, KEY ^ (1_u64 << 32)] {
        refuse(
            &mut f.engine,
            &f.ids,
            &f.probe,
            &PAGES,
            (key, 0, u32::MAX),
            HostError::InvalidArtifact,
        );
    }
    for absent in [0, u64::MAX, id ^ (1_u64 << 32)] {
        refuse(
            &mut f.engine,
            &f.ids,
            &f.probe,
            &PAGES,
            (KEY, absent, u32::MAX),
            HostError::Resident(RegistryError::InvalidUnit),
        );
    }
    for slot in [0, 2, 4, 7, 8, u32::MAX] {
        refuse(
            &mut f.engine,
            &f.ids,
            &f.probe,
            &PAGES,
            (KEY, id, slot),
            HostError::InvalidRequest,
        );
    }
    refuse(
        &mut f.engine,
        &f.ids,
        &f.probe,
        &PAGES,
        (KEY, id, 3),
        HostError::Call(CallError::Cancelled),
    );
    // cancellation is after slot validation; an empty slot does not authenticate an uninstalled id.
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    remove_exact(&mut f.engine, &f.ids, &f.probe, &PAGES, id, 3);
    let next = compile(&mut f.engine, CODE + 128, false);
    let mut ids = f.ids.to_vec();
    ids.push(next);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    refuse(
        &mut f.engine,
        &ids,
        &f.probe,
        &PAGES,
        (KEY, next, 3),
        HostError::InvalidRequest,
    );
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    f.engine.protect(CODE, 1, 7).unwrap();
    f.engine.write8(CODE, SIMPLE[0] as u32).unwrap();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    refuse(
        &mut f.engine,
        &ids,
        &f.probe,
        &PAGES,
        (KEY, next, u32::MAX),
        HostError::Resident(RegistryError::CodeInvalidated),
    );
}

const OUTER: u32 = 0x2020;
const HOME: u32 = 0x2200;
const RETURN: u32 = 0x2220;
const INNER: u32 = 0x2240;
const EXIT_GATE: u32 = 0x2260;
const ACTIVE: u32 = 0x2300;
const ACTIVE_INNER: u32 = 0x2320;

fn descriptors(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    let words = blocks
        .iter()
        .chain(gates.iter())
        .flat_map(|&(left, right)| [left, right]);
    for (index, word) in words.enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
}

fn gated_resident(
    engine: &mut EngineInstance,
    blocks: &[(u32, u32)],
    gates: &[(u32, u32)],
    slot: u32,
) -> u64 {
    descriptors(engine, blocks, gates);
    let id = engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get();
    engine
        .acknowledge_resident_installation(KEY, id, slot)
        .unwrap();
    id
}

fn stop(engine: &mut EngineInstance, pc: u32, reason: ExitReason) {
    let mut state = crate::abi::x86::decode_state(&engine.arena()[..56]).unwrap();
    state.eip = pc;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 1, reason },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn callback_fixture() -> (Fixture, [u64; 3]) {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    for pc in [CODE, REPLACEMENT, HOME, ACTIVE] {
        upload(&mut engine, pc, &SIMPLE);
    }
    for pc in [SET_ERROR, OUTER, RETURN, INNER, EXIT_GATE, ACTIVE_INNER] {
        upload(&mut engine, pc, &[0x0f, 0x0b]);
    }
    engine.protect(CODE, 1, 5).unwrap();
    engine.protect(REPLACEMENT, 1, 5).unwrap();
    engine.protect(0x8000, 1, 3).unwrap();
    let victim = gated_resident(&mut engine, &[(CODE, 3)], &[], 0);
    let outer = gated_resident(&mut engine, &[(OUTER, 2)], &[(OUTER, 17)], 1);
    let home = gated_resident(
        &mut engine,
        &[(HOME, 3), (RETURN, 2), (INNER, 2)],
        &[(RETURN, 18), (INNER, 19)],
        2,
    );
    let active = gated_resident(
        &mut engine,
        &[(ACTIVE, 3), (ACTIVE_INNER, 2)],
        &[(ACTIVE_INNER, 20)],
        3,
    );
    let blocks = [
        (REPLACEMENT, 3),
        (SET_ERROR, 2),
        (OUTER, 2),
        (HOME, 3),
        (RETURN, 2),
        (INNER, 2),
        (EXIT_GATE, 2),
    ];
    let gates = [
        (SET_ERROR, WindowsApi32::SetLastError.id()),
        (OUTER, 17),
        (RETURN, 18),
        (INNER, 19),
        (EXIT_GATE, WindowsApi32::ExitProcess.id()),
    ];
    descriptors(&mut engine, &blocks, &gates);
    engine
        .compile_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap();
    engine.write32(STACK, REPLACEMENT).unwrap();
    engine.write32(STACK + 4, 0xfeed_abcd).unwrap();
    let fp = seed(&mut engine);
    let state = crate::abi::x86::decode_state(&engine.arena()[..56]).unwrap();
    let probe = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    (
        Fixture {
            engine,
            ids: [victim, outer, home, active, 0, 0, 0, 0],
            probe,
            fp,
        },
        [outer, home, active],
    )
}

fn busy_matrix(f: &mut Fixture) {
    let ids: Vec<_> = f.ids.into_iter().filter(|&id| id != 0).collect();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    for (id, slot) in [
        (0, u32::MAX),
        (u64::MAX, 8),
        (ids[0], 0),
        (ids[1], 1),
        (ids[2], 2),
        (ids[3], 3),
    ] {
        refuse(
            &mut f.engine,
            &ids,
            &f.probe,
            &PAGES,
            (KEY, id, slot),
            HostError::Call(CallError::Busy),
        );
    }
    refuse(
        &mut f.engine,
        &ids,
        &f.probe,
        &PAGES,
        (KEY ^ 1, 0, u32::MAX),
        HostError::InvalidArtifact,
    );
    f.engine.arena_mut().unwrap()[96..100].fill(0);
}

#[test]
fn discard_installed_blocks_replacement_pending_suspended_and_inner_callback_owners() {
    for phase in 0..3 {
        let (mut f, _) = callback_fixture();
        stop(&mut f.engine, OUTER, ExitReason::Gate { id: 17 });
        let generation = f.engine.generation;
        let outer = f
            .engine
            .capture_call(KEY, generation, CallingConvention32::Stdcall, 1)
            .unwrap()
            .token;
        if phase > 0 {
            f.engine
                .begin_callback(KEY, generation, outer, HOME, RETURN, 18, &[0x1357_9bdf])
                .unwrap();
        }
        if phase > 1 {
            stop(&mut f.engine, INNER, ExitReason::Gate { id: 19 });
            f.engine
                .capture_call(KEY, generation, CallingConvention32::Stdcall, 0)
                .unwrap();
        }
        assert!(f.engine.pending_call.is_some() || f.engine.callback.is_some());
        busy_matrix(&mut f);
        // busy must also precede currency, even when a pinned unit has become stale.
        f.engine.protect(CODE, 1, 7).unwrap();
        f.engine.write8(CODE, SIMPLE[0] as u32).unwrap();
        busy_matrix(&mut f);
    }
}

#[test]
fn discard_installed_blocks_resident_prepared_authorized_active_and_inner_callback_owners() {
    for phase in 0..6 {
        let (mut f, [outer, home, active]) = callback_fixture();
        stop(&mut f.engine, OUTER, ExitReason::Gate { id: 17 });
        let outer_token = f
            .engine
            .capture_resident_call(KEY, outer, CallingConvention32::Stdcall, 1)
            .unwrap()
            .token;
        let mut callback_token = 0;
        if phase > 0 {
            callback_token = f
                .engine
                .begin_resident_callback(
                    KEY,
                    outer,
                    home,
                    outer_token,
                    HOME,
                    RETURN,
                    18,
                    &[0x2468_ace0],
                )
                .unwrap()
                .token;
        }
        if phase > 1 {
            f.engine
                .authorize_resident_callback(KEY, home, callback_token)
                .unwrap();
        }
        if phase == 3 {
            stop(&mut f.engine, INNER, ExitReason::Gate { id: 19 });
            f.engine
                .capture_resident_callback_call(
                    KEY,
                    home,
                    callback_token,
                    CallingConvention32::Stdcall,
                    0,
                )
                .unwrap();
        }
        if phase > 3 {
            stop(&mut f.engine, ACTIVE, ExitReason::NeedCode);
            f.engine
                .select_resident_callback_unit(KEY, home, callback_token, active)
                .unwrap();
        }
        if phase == 5 {
            stop(&mut f.engine, ACTIVE_INNER, ExitReason::Gate { id: 20 });
            f.engine
                .capture_active_resident_callback_call(
                    KEY,
                    active,
                    callback_token,
                    CallingConvention32::Stdcall,
                    0,
                )
                .unwrap();
        }
        busy_matrix(&mut f);
    }
}

#[test]
fn discard_installed_preserves_stale_overlap_and_reports_removed_identity_separately() {
    for entries in [false, true] {
        let mut f = fixture(entries);
        let stale = f.ids[0];
        f.engine.protect(CODE, 1, 7).unwrap();
        f.engine.write8(CODE, SIMPLE[0] as u32).unwrap();
        f.engine.protect(CODE, 1, 5).unwrap();
        // the capacity fixture is reduced through the existing stale-only reclamation API.
        for id in f.ids.into_iter().skip(1) {
            f.engine.retire_stale_resident(KEY, id).unwrap();
        }
        let current = compile(&mut f.engine, CODE, entries);
        f.engine
            .acknowledge_resident_installation(KEY, current, 1)
            .unwrap();
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, CODE),
            Ok(ResidentInstallation {
                unit_id: current,
                slot: 1
            })
        );
        remove_exact(
            &mut f.engine,
            &[stale, current],
            &f.probe,
            &PAGES,
            current,
            1,
        );
        assert_eq!(
            f.engine.guard_resident(KEY, stale),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, CODE),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        refuse(
            &mut f.engine,
            &[stale, current],
            &f.probe,
            &PAGES,
            (KEY, stale, 0),
            HostError::Resident(RegistryError::CodeInvalidated),
        );
    }
}

#[test]
fn discard_installed_checks_terminal_memory_before_all_other_owners() {
    let (mut f, _) = callback_fixture();
    stop(
        &mut f.engine,
        EXIT_GATE,
        ExitReason::Gate {
            id: WindowsApi32::ExitProcess.id(),
        },
    );
    let generation = f.engine.generation;
    let token = f
        .engine
        .capture_call(KEY, generation, CallingConvention32::Stdcall, 1)
        .unwrap()
        .token;
    f.engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(f.engine.exit_code, Some(0xfeed_abcd));
    // retained arena access here is a private terminal-state setup, never a freed module read.
    f.engine.arena.as_mut().get_mut()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    let ids: Vec<_> = f.ids.into_iter().filter(|&id| id != 0).collect();
    for request in [(0, 0, u32::MAX), (KEY, ids[0], 0)] {
        refuse(
            &mut f.engine,
            &ids,
            &f.probe,
            &PAGES,
            request,
            HostError::ProcessExited,
        );
    }
    f.engine.close();
    assert!(!f.engine.is_open());
    for request in [(0, 0, u32::MAX), (KEY, ids[0], 0)] {
        refuse(
            &mut f.engine,
            &ids,
            &f.probe,
            &PAGES,
            request,
            HostError::Closed,
        );
    }
    f.engine.close();
    refuse(
        &mut f.engine,
        &ids,
        &f.probe,
        &PAGES,
        (KEY, ids[0], 0),
        HostError::Closed,
    );
}

#[test]
fn discard_installed_handles_empty_registry_and_preserves_occupied_optional_owners() {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    engine.begin_image_input(8).unwrap();
    engine.append_image_input(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!(
        engine.append_image_input(3, &[5]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.append_image_input(4, &[5, 6, 7, 8, 9]),
        Err(HostError::InvalidRequest)
    );
    for page in PAGES {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, CODE, &SIMPLE);
    engine.protect(CODE, 1, 5).unwrap();
    engine.protect(REPLACEMENT, 1, 5).unwrap();
    engine.protect(0x8000, 1, 3).unwrap();
    engine.write32(STACK, CODE).unwrap();
    seed(&mut engine);
    let probe = CallFrame32::capture(
        engine.memory().unwrap(),
        crate::abi::x86::decode_state(&engine.arena()[..56]).unwrap(),
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    refuse(
        &mut engine,
        &[],
        &probe,
        &PAGES,
        (KEY, 0, u32::MAX),
        HostError::Resident(RegistryError::InvalidUnit),
    );
    refuse(
        &mut engine,
        &[],
        &probe,
        &PAGES,
        (KEY ^ 1, 0, u32::MAX),
        HostError::InvalidArtifact,
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    let id = compile(&mut engine, CODE, false);
    engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    // typed owner occupancy tests preservation without exercising the unrelated PE loader.
    engine.image = Some(crate::loader::ImageMetadata32 {
        image_base: CODE,
        image_size: 4096,
        entry_point: CODE,
        mapped_pages: 1,
    });
    engine.image_started = true;
    engine
        .virtual_allocations
        .push(PageRange::new(GuestAddress(0x8000), 1).unwrap());
    let input_address = engine.image_input.as_ref().unwrap() as *const _ as usize;
    remove_exact(&mut engine, &[id], &probe, &PAGES, id, 0);
    assert_eq!(
        engine.image_input.as_ref().unwrap() as *const _ as usize,
        input_address
    );
    // public append is intentionally no longer eligible after mappings/publication exist.
    let before = observe(&engine, &[id], &probe, &PAGES);
    assert_eq!(
        engine.append_image_input(4, &[5, 6, 7, 8]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(observe(&engine, &[id], &probe, &PAGES), before);
    engine.abort_image_input().unwrap();
    assert!(engine.image_input.is_none());
    refuse(
        &mut engine,
        &[id],
        &probe,
        &PAGES,
        (KEY, id, 0),
        HostError::Resident(RegistryError::InvalidUnit),
    );
}
