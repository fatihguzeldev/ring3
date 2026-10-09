use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE, X87_OFFSET},
        resident_callback::ResidentCallbackRecord32,
        x86::{X87_SIZE, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{CodeSnapshot, GuestAddress},
    process::{CallError, EngineInstance, HostError, ResidentInstallation, StoreCompletion},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const OUTER: u32 = 0x1000;
const ENTRY: u32 = 0x2000;
const RETURN: u32 = 0x2100;
const CALLER: u32 = 0x3000;
const PAGES: [u32; 5] = [OUTER, ENTRY, CALLER, 0x8000, 0x9000];

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    callback: u64,
    foreign: u64,
    generation: u32,
    esp: u32,
    outer_x87: [u8; X87_SIZE],
    snapshots: Vec<CodeSnapshot>,
}
fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}
fn descriptors(engine: &mut EngineInstance, pairs: &[(u32, u32)]) {
    for (index, (pc, value)) in pairs.iter().enumerate() {
        let arena = engine.arena_mut().unwrap();
        arena[140 + index * 8..144 + index * 8].copy_from_slice(&pc.to_le_bytes());
        arena[144 + index * 8..148 + index * 8].copy_from_slice(&value.to_le_bytes());
    }
}
fn fixture(esp: u32, same_unit: bool, installed_callback: bool) -> Fixture {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for pc in PAGES {
        engine.map(pc, 1, 7).unwrap();
    }
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90]);
    upload(&mut engine, RETURN, &[0x0f, 0x0b]);
    upload(&mut engine, CALLER, &[0x90]);
    engine.write32(esp, CALLER).unwrap();
    let all = [
        (OUTER, 2),
        (ENTRY, 1),
        (RETURN, 2),
        (OUTER, 17),
        (RETURN, 18),
    ];
    descriptors(&mut engine, &all);
    let generation = engine.compile_with_gates(3, 2).unwrap();
    assert_eq!(generation, 1);
    let (outer, callback) = if same_unit {
        descriptors(&mut engine, &all);
        let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
        (id, id)
    } else {
        descriptors(&mut engine, &[(OUTER, 2), (OUTER, 17)]);
        let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
        descriptors(&mut engine, &[(ENTRY, 1), (RETURN, 2), (RETURN, 18)]);
        (
            outer,
            engine.compile_resident_with_gates(2, 1).unwrap().get(),
        )
    };
    descriptors(&mut engine, &[(CALLER, 1)]);
    let foreign = engine.compile_resident(1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, outer, 0)
        .unwrap();
    if installed_callback && callback != outer {
        engine
            .acknowledge_resident_installation(KEY, callback, 1)
            .unwrap();
    }
    engine
        .acknowledge_resident_installation(KEY, foreign, 2)
        .unwrap();
    let snapshots = PAGES
        .iter()
        .map(|&pc| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(pc), 4096)
                .unwrap()
        })
        .collect();
    let outer_x87 = engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]
        .try_into()
        .unwrap();
    Fixture {
        engine,
        outer,
        callback,
        foreign,
        generation,
        esp,
        outer_x87,
        snapshots,
    }
}
fn frozen(esp: u32) -> State32 {
    State32 {
        registers: [
            7,
            0x5566_7788,
            0x99aa_bbcc,
            0xddee_ff00,
            esp,
            0x1234_5678,
            0x2345_6789,
            0x3456_789a,
        ],
        eip: OUTER,
        eflags: 0xcd7,
    }
}
fn capture(f: &mut Fixture, resident: bool) -> u32 {
    // typed native Gate controls; these tests do not execute guest instructions.
    let arena = f.engine.arena_mut().unwrap();
    encode_state(&frozen(f.esp), &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    if resident {
        f.engine
            .capture_resident_call(KEY, f.outer, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    } else {
        f.engine
            .capture_call(KEY, f.generation, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    }
}
fn begin(f: &mut Fixture, token: u32, args: &[u32]) -> ResidentCallbackRecord32 {
    f.engine
        .begin_resident_callback(KEY, f.outer, f.callback, token, ENTRY, RETURN, 18, args)
        .unwrap()
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
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn state_literal(state: &State32) -> Vec<u8> {
    let mut fields = vec![u32::from_le_bytes(*b"R3ST"), 0x10001, 56, 0];
    fields.extend(state.registers);
    fields.extend([state.eip, state.eflags]);
    words(&fields)
}
fn exit_literal(reason: u32, retired: u32, detail: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3EX"),
        0x10003,
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
fn record_literal(f: &Fixture, token: u32, outer: u32, args: usize, outcome: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3RC"),
        0x10001,
        72,
        0,
        token,
        outer,
        1,
        outcome,
        ENTRY,
        f.esp - 4 * (args as u32 + 1),
        RETURN,
        18,
        args as u32,
        0,
        f.outer as u32,
        (f.outer >> 32) as u32,
        f.callback as u32,
        (f.callback >> 32) as u32,
    ])
}
fn helper_literal(fields: [u32; 6]) -> Vec<u8> {
    let mut header = vec![u32::from_le_bytes(*b"R3MH"), 0x10001, 40, 0];
    header.extend(fields);
    words(&header)
}
fn read_word(engine: &EngineInstance, pc: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(pc), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

type Module = Result<(Vec<u8>, usize), HostError>;
#[derive(Debug, PartialEq, Eq)]
struct Storage {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    modules: Vec<Module>,
    logical: Vec<Result<u64, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    snapshots: Vec<Result<bool, HostError>>,
}
fn storage(f: &Fixture) -> Storage {
    let copy = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    Storage {
        arena: f.engine.arena().to_vec(),
        arena_address: f.engine.arena_address(),
        generation: f.engine.generation(),
        modules: vec![
            f.engine.dispatcher_bytes(KEY).map(copy),
            f.engine.artifact_bytes().map(copy),
            f.engine.resident_bytes(f.outer).map(copy),
            f.engine.resident_bytes(f.callback).map(copy),
            f.engine.resident_bytes(f.foreign).map(copy),
        ],
        logical: [OUTER, ENTRY, RETURN, CALLER]
            .map(|pc| f.engine.lookup_resident(pc).map(|id| id.get()))
            .to_vec(),
        ram: PAGES
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
        snapshots: f
            .snapshots
            .iter()
            .map(|snapshot| Ok(f.engine.memory()?.is_code_current(snapshot)))
            .collect(),
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
            f.engine.guard_resident(KEY, f.callback),
            f.engine.guard_resident(KEY, f.foreign),
        ],
        installed: [OUTER, ENTRY, RETURN, CALLER]
            .map(|pc| f.engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
    }
}
fn reject(f: &mut Fixture, key: u64, id: u64, token: u32, error: HostError) {
    let before = observe(f);
    assert_eq!(
        f.engine.authorize_resident_callback(key, id, token),
        Err(error)
    );
    assert_eq!(observe(f), before);
}
fn abort_restores(f: &mut Fixture, token: u32) {
    let before = storage(f);
    f.engine.abort_callback(KEY, token).unwrap();
    let mut expected = before.arena.clone();
    expected[..56].copy_from_slice(&state_literal(&frozen(f.esp)));
    expected[56..96].copy_from_slice(&exit_literal(8, 1, 17));
    expected[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&f.outer_x87);
    let after = storage(f);
    assert_eq!(after.arena, expected);
    assert_eq!(after.arena_address, before.arena_address);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.modules, before.modules);
    assert_eq!(after.logical, before.logical);
    assert_eq!(after.ram, before.ram);
    assert_eq!(after.snapshots, before.snapshots);
}

#[test]
fn resident_callback_authorization_is_pure_and_opens_only_its_named_unit() {
    let mut f = fixture(0x8004, false, true);
    let outer_token = capture(&mut f, true);
    assert_eq!(outer_token, 1);
    let before_begin = storage(&f);
    let record = begin(&mut f, outer_token, &[]);
    assert_eq!(
        (record.token, record.outer_token, record.outcome),
        (2, 1, 0)
    );
    let mut expected = before_begin.arena.clone();
    let mut entered = frozen(f.esp);
    entered.eip = ENTRY;
    entered.registers[4] = 0x8000;
    expected[..56].copy_from_slice(&state_literal(&entered));
    expected[56..96].copy_from_slice(&exit_literal(3, 0, 0));
    expected[140..212].copy_from_slice(&record_literal(&f, 2, 1, 0, 0));
    assert_eq!(f.engine.arena(), expected);
    let mut ram = before_begin.ram;
    ram[3].as_mut().unwrap()[..4].copy_from_slice(&RETURN.to_le_bytes());
    assert_eq!(storage(&f).ram, ram);
    let bindings = observe(&f).installed;
    assert_eq!(observe(&f).guards, vec![Err(call(CallError::Busy)); 5]);
    // private admission is authoritative even when mutable transfer and stack are forged.
    f.engine.write32(0x8000, 0xdead_beef).unwrap();
    f.engine.write32(0x8004, 0xbad0_cafe).unwrap();
    f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0x5a);
    let before = storage(&f);
    assert_eq!(
        f.engine
            .authorize_resident_callback(KEY, f.callback, record.token),
        Ok(())
    );
    assert_eq!(storage(&f), before);
    assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
    assert_eq!(
        observe(&f).guards,
        vec![
            Ok(()),
            Err(call(CallError::Busy)),
            Err(call(CallError::Busy)),
            Ok(()),
            Err(call(CallError::Busy))
        ]
    );
    assert_eq!(f.engine.lookup_installed_resident(KEY, ENTRY), bindings[1]);
    reject(
        &mut f,
        KEY,
        record.callback_unit_id,
        record.token,
        call(CallError::Busy),
    );
    // typed advanced CPU is legal for execution, but reauthorization cannot reset it.
    f.engine.arena_mut().unwrap()[..96].fill(0xff);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(
        &mut f,
        KEY,
        record.callback_unit_id,
        record.token,
        call(CallError::Busy),
    );
    let before = observe(&f);
    assert_eq!(
        f.engine.abort_callback(KEY, outer_token),
        Err(call(CallError::InvalidToken))
    );
    assert_eq!(observe(&f), before);
    abort_restores(&mut f, record.token);
    assert_eq!(observe(&f).installed, bindings);
    assert_eq!(
        f.engine.guard_resident(KEY, f.callback),
        Err(call(CallError::Busy))
    );
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    let next = begin(&mut f, outer_token, &[]);
    assert_eq!(next.token, 3);
    assert_eq!(
        f.engine.guard_dispatch_entry(KEY),
        Err(call(CallError::Busy))
    );
    assert_eq!(
        f.engine.guard_resident(KEY, f.callback),
        Err(call(CallError::Busy))
    );
    reject(
        &mut f,
        KEY,
        record.callback_unit_id,
        record.token,
        call(CallError::InvalidToken),
    );
    f.engine
        .authorize_resident_callback(KEY, f.callback, next.token)
        .unwrap();
    abort_restores(&mut f, next.token);
    f.engine
        .complete_resident_call(KEY, f.outer, outer_token, 55)
        .unwrap();
    let mut completed = frozen(f.esp);
    completed.registers[0] = 55;
    completed.registers[4] = 0x8008;
    completed.eip = CALLER;
    assert_eq!(&f.engine.arena()[..56], state_literal(&completed));
    assert_eq!(&f.engine.arena()[56..96], exit_literal(3, 0, 0));
    // new capture reads the currently forged caller word; begin still starts unarmed.
    let final_outer = capture(&mut f, true);
    assert_eq!(final_outer, 4);
    let final_callback = begin(&mut f, final_outer, &[]);
    assert_eq!(final_callback.token, 5);
    assert_eq!(
        f.engine.guard_dispatch_entry(KEY),
        Err(call(CallError::Busy))
    );
}

#[test]
fn authorization_priorities_keep_family_identity_frozen_cpu_and_currency_exact() {
    let mut f = fixture(0x8004, false, true);
    let callback = f.callback;
    reject(&mut f, KEY, callback, 1, call(CallError::InvalidToken));
    let token = capture(&mut f, true);
    reject(&mut f, KEY, callback, token, call(CallError::InvalidToken));
    let record = begin(&mut f, token, &[]);
    for (key, id, token, error) in [
        (
            KEY ^ (1_u64 << 32),
            callback,
            record.token,
            HostError::InvalidArtifact,
        ),
        (KEY as u32 as u64, 0, 0, HostError::InvalidArtifact),
        (KEY, 0, 0, invalid_unit()),
        (KEY, callback ^ (1_u64 << 32), 0, invalid_unit()),
        (KEY, u64::MAX, record.token, invalid_unit()),
        (KEY, f.outer, record.token, call(CallError::InvalidToken)),
        (KEY, f.foreign, record.token, call(CallError::InvalidToken)),
        (KEY, callback, 0, call(CallError::InvalidToken)),
        (KEY, callback, token, call(CallError::InvalidToken)),
        (KEY, callback, u32::MAX, call(CallError::InvalidToken)),
    ] {
        reject(&mut f, key, id, token, error);
    }
    let saved = f.engine.arena()[..100].to_vec();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    for offset in [
        0, 4, 8, 12, 16, 32, 48, 52, 56, 60, 64, 68, 72, 76, 80, 84, 88, 92,
    ] {
        f.engine.arena_mut().unwrap()[offset] ^= 1;
        reject(
            &mut f,
            KEY,
            callback,
            record.token,
            call(CallError::StateChanged),
        );
        reject(&mut f, KEY, callback, 0, call(CallError::InvalidToken));
        f.engine.arena_mut().unwrap()[offset] ^= 1;
    }
    reject(
        &mut f,
        KEY,
        callback,
        record.token,
        call(CallError::Cancelled),
    );
    f.engine.arena_mut().unwrap()[..100].copy_from_slice(&saved);
    f.engine
        .authorize_resident_callback(KEY, callback, record.token)
        .unwrap();
    reject(
        &mut f,
        KEY ^ (1_u64 << 32),
        callback,
        record.token,
        HostError::InvalidArtifact,
    );
    reject(&mut f, KEY, callback, 0, call(CallError::InvalidToken));

    let mut replacement = fixture(0x8004, false, true);
    let replacement_outer = capture(&mut replacement, false);
    let legacy = replacement
        .engine
        .begin_callback(
            KEY,
            replacement.generation,
            replacement_outer,
            ENTRY,
            RETURN,
            18,
            &[],
        )
        .unwrap();
    let replacement_id = replacement.callback;
    reject(
        &mut replacement,
        KEY,
        replacement_id,
        legacy.token,
        call(CallError::InvalidToken),
    );
    assert_eq!(
        replacement.engine.guard(KEY, replacement.generation),
        Ok(())
    );
    assert_eq!(
        replacement.engine.guard_resident(KEY, replacement_id),
        Err(call(CallError::Busy))
    );

    let mut same = fixture(0x8004, true, true);
    let same_outer = capture(&mut same, true);
    let same_record = begin(&mut same, same_outer, &[]);
    same.engine.arena_mut().unwrap()[..96].fill(0xff);
    same.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    let same_id = same.callback;
    reject(
        &mut same,
        KEY,
        same_id,
        same_record.token,
        call(CallError::InvalidRequest),
    );
    abort_restores(&mut same, same_record.token);

    for (esp, args, outcome) in [
        (0x2104, vec![], 1),
        (0x1104, vec![], 2),
        (0x2004, vec![0x1122_3344, 0x5566_7788], 3),
    ] {
        let mut invalidated = fixture(esp, false, true);
        let token = capture(&mut invalidated, true);
        let record = begin(&mut invalidated, token, &args);
        assert_eq!(record.outcome, outcome);
        assert_eq!(
            &invalidated.engine.arena()[140..212],
            record_literal(&invalidated, 2, 1, args.len(), outcome)
        );
        invalidated.engine.arena_mut().unwrap()[..96].fill(0xff);
        invalidated.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        let id = invalidated.callback;
        reject(&mut invalidated, KEY, id, record.token, stale());
        reject(
            &mut invalidated,
            KEY,
            id,
            0,
            if outcome == 2 {
                call(CallError::InvalidToken)
            } else {
                stale()
            },
        );
        abort_restores(&mut invalidated, record.token);
    }
    for target in [OUTER, ENTRY] {
        let mut invalidated = fixture(0x8004, false, true);
        let token = capture(&mut invalidated, true);
        let record = begin(&mut invalidated, token, &[]);
        let bytes = read_word(&invalidated.engine, target);
        invalidated.engine.write32(target, bytes).unwrap();
        let id = invalidated.callback;
        reject(&mut invalidated, KEY, id, record.token, stale());
        reject(
            &mut invalidated,
            KEY,
            id,
            0,
            if target == OUTER {
                call(CallError::InvalidToken)
            } else {
                stale()
            },
        );
        abort_restores(&mut invalidated, record.token);
    }
    let before = f.engine.arena().to_vec();
    f.engine.close();
    assert_eq!(f.engine.arena(), before);
    reject(&mut f, 0, 0, 0, HostError::Closed);
}

#[test]
fn armed_execution_boundaries_exclude_foreign_units_capture_installation_and_legacy() {
    for installed in [true, false] {
        let mut f = fixture(0x8004, false, installed);
        let token = capture(&mut f, true);
        let record = begin(&mut f, token, &[]);
        f.engine
            .authorize_resident_callback(KEY, f.callback, record.token)
            .unwrap();
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
        let before = observe(&f);
        for pc in [ENTRY, RETURN] {
            assert_eq!(
                f.engine.lookup_installed_resident(KEY, pc),
                if installed {
                    Ok(ResidentInstallation {
                        unit_id: f.callback,
                        slot: 1,
                    })
                } else {
                    Err(missing(pc))
                }
            );
        }
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, OUTER),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, CALLER),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, ENTRY + 1),
            Err(missing(ENTRY + 1))
        );
        assert_eq!(
            f.engine
                .lookup_installed_resident(KEY ^ (1_u64 << 32), OUTER),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(f.engine.lookup_resident(OUTER).unwrap().get(), f.outer);
        assert_eq!(f.engine.lookup_resident(CALLER).unwrap().get(), f.foreign);
        assert_eq!(observe(&f), before);
        // advanced/forged native CPU cannot broaden identity or admit host-side inner work.
        f.engine.arena_mut().unwrap()[..96].fill(0xff);
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        let before = observe(&f);
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
        assert_eq!(
            f.engine.guard_resident(KEY, f.outer),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.guard_resident(KEY, f.foreign),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.guard(KEY, f.generation),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.guard_resident(KEY, f.callback ^ (1_u64 << 32)),
            Err(invalid_unit())
        );
        for id in [f.callback, f.outer, f.foreign] {
            assert_eq!(
                f.engine.capture_resident_call_raw(KEY, id, 0, u32::MAX),
                Err(call(CallError::Busy))
            );
            assert_eq!(
                f.engine
                    .acknowledge_resident_installation(KEY, id, u32::MAX),
                Err(call(CallError::Busy))
            );
        }
        for id in [f.outer, f.foreign] {
            assert_eq!(
                f.engine.store_resident32(KEY, id, u32::MAX, 0),
                Err(call(CallError::Busy))
            );
        }
        assert_eq!(
            f.engine.capture_resident_call_raw(KEY, 0, 0, u32::MAX),
            Err(invalid_unit())
        );
        assert_eq!(
            f.engine.acknowledge_resident_installation(KEY, 0, u32::MAX),
            Err(invalid_unit())
        );
        assert_eq!(
            f.engine.capture_call_raw(KEY, f.generation, 0, u32::MAX),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.compile_with_gates(0, 0),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.compile_resident_with_gates(0, 0),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine
                .begin_resident_callback(KEY, f.outer, f.callback, 0, 0, 0, 0, &[0; 17]),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine
                .begin_callback(KEY, f.generation, 0, 0, 0, 0, &[0; 17]),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.complete_resident_call(KEY, f.outer, token, 55),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.abandon_call(KEY, token),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.finish_callback(KEY, f.generation, record.token),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(
            f.engine
                .resume_callback_code(KEY, f.generation, record.token, 0, 0),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(
            f.engine
                .resume_callback_entries(KEY, f.generation, record.token, 0, 0),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(observe(&f), before);
        let foreign_word = read_word(&f.engine, CALLER);
        f.engine.write32(CALLER, foreign_word).unwrap();
        let before = observe(&f);
        assert_eq!(
            f.engine.lookup_installed_resident(KEY, CALLER),
            Err(stale())
        );
        assert_eq!(f.engine.guard_resident(KEY, f.foreign), Err(stale()));
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
        assert_eq!(observe(&f), before);
        // a typed registered return stop remains armed without publishing a result or outer authority.
        let mut returned = frozen(f.esp);
        returned.eip = RETURN;
        returned.registers[0] = 99;
        f.engine.arena_mut().unwrap()[..56].copy_from_slice(&state_literal(&returned));
        f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&exit_literal(8, 1, 18));
        assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        reject(
            &mut f,
            KEY,
            record.callback_unit_id,
            record.token,
            call(CallError::Busy),
        );
        abort_restores(&mut f, record.token);
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        f.engine
            .complete_resident_call(KEY, f.outer, token, 55)
            .unwrap();
        assert_eq!(read_word(&f.engine, 0x8000), RETURN);
        assert_eq!(
            u32::from_le_bytes(f.engine.arena()[16..20].try_into().unwrap()),
            55
        );
        let before = f.engine.arena().to_vec();
        f.engine.close();
        assert_eq!(f.engine.arena(), before);
        let before = observe(&f);
        assert_eq!(f.engine.guard_dispatch_entry(0), Err(HostError::Closed));
        assert_eq!(f.engine.guard_resident(0, 0), Err(HostError::Closed));
        assert_eq!(
            f.engine.store_resident32(0, 0, u32::MAX, 0),
            Err(HostError::Closed)
        );
        assert_eq!(
            f.engine.lookup_installed_resident(0, 0),
            Err(HostError::Closed)
        );
        assert_eq!(f.engine.abort_callback(0, 0), Err(HostError::Closed));
        assert_eq!(observe(&f), before);
    }
}

#[test]
fn armed_store_commits_only_its_snapshot_check_and_abort_preserves_stale_recovery() {
    for target in [0x9000, RETURN, OUTER] {
        let mut f = fixture(0x8004, false, true);
        let token = capture(&mut f, true);
        let record = begin(&mut f, token, &[]);
        f.engine
            .authorize_resident_callback(KEY, f.callback, record.token)
            .unwrap();
        let value = if target == 0x9000 {
            0x4433_2211
        } else {
            read_word(&f.engine, target)
        };
        let before = storage(&f);
        assert_eq!(
            f.engine.store_resident32(KEY, f.callback, target, value),
            Ok(if target == RETURN {
                StoreCompletion::CodeInvalidated
            } else {
                StoreCompletion::Complete
            })
        );
        let mut expected = before.arena.clone();
        expected[100..140].copy_from_slice(&helper_literal([0; 6]));
        assert_eq!(f.engine.arena(), expected);
        let mut ram = before.ram;
        let index = PAGES
            .iter()
            .position(|&page| page == (target & !0xfff))
            .unwrap();
        let offset = (target & 0xfff) as usize;
        ram[index].as_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert_eq!(storage(&f).ram, ram);
        assert!(
            !f.engine
                .memory()
                .unwrap()
                .is_code_current(&f.snapshots[index])
        );
        assert_eq!(f.engine.arena_address(), before.arena_address);
        assert_eq!(f.engine.generation(), before.generation);
        assert_eq!(
            f.engine.dispatcher_bytes(KEY).unwrap(),
            before.modules[0].as_ref().unwrap().0
        );
        assert_eq!(
            f.engine.dispatcher_bytes(KEY).unwrap().as_ptr() as usize,
            before.modules[0].as_ref().unwrap().1
        );
        assert_eq!(
            f.engine.resident_bytes(f.foreign).unwrap(),
            before.modules[4].as_ref().unwrap().0
        );
        if target == 0x9000 {
            assert_eq!(storage(&f).modules, before.modules);
            assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
            let snapshot = f
                .engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x9000), 4096)
                .unwrap();
            let before_fault = storage(&f);
            assert_eq!(
                f.engine.store_resident32(KEY, f.callback, 0x9fff, u32::MAX),
                Ok(StoreCompletion::Complete)
            );
            let mut expected = before_fault.arena.clone();
            expected[100..140].copy_from_slice(&helper_literal([1, 0, 1, 0xa000, 2, 4]));
            assert_eq!(f.engine.arena(), expected);
            assert_eq!(storage(&f).ram, before_fault.ram);
            assert_eq!(storage(&f).modules, before_fault.modules);
            assert!(f.engine.memory().unwrap().is_code_current(&snapshot));
        } else {
            assert_eq!(f.engine.guard_dispatch_entry(KEY), Err(stale()));
            assert_eq!(f.engine.guard_resident(KEY, f.callback), Err(stale()));
            reject(&mut f, KEY, record.callback_unit_id, record.token, stale());
            reject(
                &mut f,
                KEY,
                record.callback_unit_id,
                0,
                if target == OUTER {
                    call(CallError::InvalidToken)
                } else {
                    stale()
                },
            );
            let before = observe(&f);
            assert_eq!(
                f.engine.store_resident32(KEY, f.callback, u32::MAX, 0),
                Err(stale())
            );
            let admission_error = if target == RETURN {
                stale()
            } else {
                call(CallError::Busy)
            };
            assert_eq!(
                f.engine
                    .capture_resident_call_raw(KEY, f.callback, 0, u32::MAX),
                Err(admission_error)
            );
            assert_eq!(
                f.engine
                    .acknowledge_resident_installation(KEY, f.callback, u32::MAX),
                Err(admission_error)
            );
            assert_eq!(observe(&f), before);
            assert_eq!(
                f.engine.guard(KEY, f.generation),
                Err(HostError::CodeInvalidated)
            );
            if target == RETURN {
                assert_eq!(f.engine.resident_bytes(f.callback), Err(stale()));
                assert_eq!(
                    f.engine.lookup_installed_resident(KEY, RETURN),
                    Err(stale())
                );
                assert_eq!(
                    f.engine.resident_bytes(f.outer).unwrap(),
                    before.storage.modules[2].as_ref().unwrap().0
                );
            } else {
                assert_eq!(f.engine.resident_bytes(f.outer), Err(stale()));
                assert_eq!(f.engine.lookup_installed_resident(KEY, OUTER), Err(stale()));
                assert_eq!(
                    f.engine.lookup_installed_resident(KEY, ENTRY),
                    Ok(ResidentInstallation {
                        unit_id: f.callback,
                        slot: 1
                    })
                );
                assert_eq!(
                    f.engine.resident_bytes(f.callback).unwrap(),
                    before.storage.modules[3].as_ref().unwrap().0
                );
                assert_eq!(
                    f.engine.resident_bytes(f.callback).unwrap().as_ptr() as usize,
                    before.storage.modules[3].as_ref().unwrap().1
                );
            }
        }
        f.engine.arena_mut().unwrap()[..96].fill(0xff);
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        f.engine.arena_mut().unwrap()[140..].fill(0xa5);
        abort_restores(&mut f, record.token);
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        if target == OUTER {
            let before = observe(&f);
            assert_eq!(
                f.engine.complete_resident_call(KEY, f.outer, token, 55),
                Err(stale())
            );
            assert_eq!(observe(&f), before);
            f.engine.abandon_call(KEY, token).unwrap();
            assert_eq!(f.engine.guard_resident(KEY, f.callback), Ok(()));
        } else {
            f.engine
                .complete_resident_call(KEY, f.outer, token, 55)
                .unwrap();
            assert_eq!(
                u32::from_le_bytes(f.engine.arena()[16..20].try_into().unwrap()),
                55
            );
        }
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        assert_eq!(read_word(&f.engine, target), value);
    }
}
