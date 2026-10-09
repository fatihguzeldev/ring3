use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        resident_callback::ResidentCallbackRecord32,
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{CodeSnapshot, GuestAddress},
    process::{CallError, EngineInstance, HostError, ResidentInstallation, StoreCompletion},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn descriptors(engine: &mut EngineInstance, pairs: &[(u32, u32)]) {
    for (index, (pc, value)) in pairs.iter().enumerate() {
        engine.arena_mut().unwrap()[140 + index * 8..144 + index * 8]
            .copy_from_slice(&pc.to_le_bytes());
        engine.arena_mut().unwrap()[144 + index * 8..148 + index * 8]
            .copy_from_slice(&value.to_le_bytes());
    }
}

#[test]
fn resident_finish_delivers_literal_result_and_restores_exact_private_outer() {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let pages = [0x1000, 0x2000, 0x3000, 0x8000, 0x9000];
    for pc in pages {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x2000, &[0x90][..]),
        (0x2100, &[0x0f, 0x0b][..]),
        (0x3000, &[0x90][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.write32(0x9000, 0x3000).unwrap();
    descriptors(&mut engine, &[(0x1000, 2), (0x1000, 17)]);
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    descriptors(&mut engine, &[(0x2000, 1), (0x2100, 2), (0x2100, 18)]);
    let callback = engine.compile_resident_with_gates(2, 1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, outer, 0)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, callback, 1)
        .unwrap();
    let frozen = State32 {
        registers: [
            7,
            0x5566_7788,
            0x99aa_bbcc,
            0xddee_ff00,
            0x9000,
            0x1234_5678,
            0x2345_6789,
            0x3456_789a,
        ],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    // typed native Gate controls; this test does not execute guest instructions.
    encode_state(&frozen, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[96..100].fill(0);
    let frozen_bytes = engine.arena()[..96].to_vec();
    let call = engine
        .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(call.token, 1);
    let admitted = engine
        .begin_resident_callback(KEY, outer, callback, call.token, 0x2000, 0x2100, 18, &[])
        .unwrap();
    assert_eq!(admitted.token, 2);
    engine
        .authorize_resident_callback(KEY, callback, admitted.token)
        .unwrap();
    let mut returned = frozen;
    returned.eip = 0x2100;
    returned.registers[0] = 42;
    returned.registers[1] = 0xffff_ffff;
    returned.eflags = 2;
    encode_state(&returned, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 18 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0x6d);
    let before = engine.arena().to_vec();
    let ram = pages.map(|pc| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(pc), &mut bytes)
            .unwrap();
        bytes
    });
    let result = engine
        .finish_resident_callback(KEY, callback, admitted.token)
        .unwrap();
    assert_eq!(result.result, 42);
    assert_eq!(
        (
            result.token,
            result.outer_token,
            result.outer_unit_id,
            result.callback_unit_id
        ),
        (2, 1, outer, callback)
    );
    let literal = words(&[
        u32::from_le_bytes(*b"R3RR"),
        0x10001,
        48,
        0,
        2,
        1,
        42,
        0,
        outer as u32,
        (outer >> 32) as u32,
        callback as u32,
        (callback >> 32) as u32,
    ]);
    let mut wanted = before;
    wanted[..96].copy_from_slice(&frozen_bytes);
    wanted[140..188].copy_from_slice(&literal);
    assert_eq!(engine.arena(), wanted);
    assert_eq!(
        pages.map(|pc| {
            let mut bytes = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut bytes)
                .unwrap();
            bytes
        }),
        ram
    );
    assert_eq!(
        engine.guard_resident(KEY, callback),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.abort_callback(KEY, admitted.token),
        Err(HostError::Call(CallError::InvalidToken))
    );
    engine
        .complete_resident_call(KEY, outer, call.token, result.result)
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(engine.arena()[16..20].try_into().unwrap()),
        42
    );
    assert_eq!(
        u32::from_le_bytes(engine.arena()[32..36].try_into().unwrap()),
        0x9004
    );
    assert_eq!(
        u32::from_le_bytes(engine.arena()[48..52].try_into().unwrap()),
        0x3000
    );
}

const OUTER: u32 = 0x1000;
const ENTRY: u32 = 0x2000;
const RETURN: u32 = 0x2100;
const CALLER: u32 = 0x3000;
const PAGES: [u32; 7] = [OUTER, ENTRY, CALLER, 0x8000, 0x9000, 0, 0xffff_f000];

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    callback: u64,
    foreign: u64,
    generation: u32,
    esp: u32,
    snapshots: Vec<CodeSnapshot>,
}

fn fixture(esp: u32, same_unit: bool) -> Fixture {
    let mut engine = EngineInstance::new(7, KEY).unwrap();
    for pc in PAGES {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (OUTER, &[0x0f, 0x0b][..]),
        (ENTRY, &[0x90][..]),
        (RETURN, &[0x0f, 0x0b][..]),
        (CALLER, &[0x90][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
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
    if outer != callback {
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
    Fixture {
        engine,
        outer,
        callback,
        foreign,
        generation,
        esp,
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
fn state_literal(s: &State32) -> Vec<u8> {
    let mut fields = vec![u32::from_le_bytes(*b"R3ST"), 0x10001, 56, 0];
    fields.extend(s.registers);
    fields.extend([s.eip, s.eflags]);
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
fn result_literal(f: &Fixture, token: u32, outer_token: u32, result: u32) -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3RR"),
        0x10001,
        48,
        0,
        token,
        outer_token,
        result,
        0,
        f.outer as u32,
        (f.outer >> 32) as u32,
        f.callback as u32,
        (f.callback >> 32) as u32,
    ])
}
fn capture(f: &mut Fixture, resident: bool) -> u32 {
    // typed Gate controls establish native ownership without executing guest code.
    f.engine.arena_mut().unwrap()[..56].copy_from_slice(&state_literal(&frozen(f.esp)));
    f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&exit_literal(8, 7, 17));
    f.engine.arena_mut().unwrap()[96..100].fill(0);
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
fn begin(f: &mut Fixture, outer_token: u32, args: &[u32], armed: bool) -> ResidentCallbackRecord32 {
    let r = f
        .engine
        .begin_resident_callback(
            KEY,
            f.outer,
            f.callback,
            outer_token,
            ENTRY,
            RETURN,
            18,
            args,
        )
        .unwrap();
    if armed {
        f.engine
            .authorize_resident_callback(KEY, f.callback, r.token)
            .unwrap();
    }
    r
}
fn stop(f: &mut Fixture, result: u32, retired: u32) {
    let mut s = frozen(f.esp);
    s.eip = RETURN;
    s.registers[0] = result;
    for index in [1, 2, 3, 5, 6, 7] {
        s.registers[index] ^= 0xffff_ffff;
    }
    s.eflags = 2;
    f.engine.arena_mut().unwrap()[..56].copy_from_slice(&state_literal(&s));
    f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&exit_literal(8, retired, 18));
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
fn read_word(f: &Fixture, pc: u32) -> u32 {
    let mut bytes = [0; 4];
    f.engine
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
    address: usize,
    generation: u32,
    modules: Vec<Module>,
    logical: Vec<Result<u64, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    snapshots: Vec<Result<bool, HostError>>,
}
fn storage(f: &Fixture) -> Storage {
    let copy = |b: &[u8]| (b.to_vec(), b.as_ptr() as usize);
    Storage {
        arena: f.engine.arena().to_vec(),
        address: f.engine.arena_address(),
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
                let mut b = vec![0; 4096];
                f.engine
                    .memory()?
                    .read(GuestAddress(pc), &mut b)
                    .map_err(HostError::Memory)?;
                Ok(b)
            })
            .collect(),
        snapshots: f
            .snapshots
            .iter()
            .map(|s| Ok(f.engine.memory()?.is_code_current(s)))
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
        f.engine.finish_resident_callback(key, id, token),
        Err(error)
    );
    assert_eq!(observe(f), before);
}
fn abort_restores(f: &mut Fixture, token: u32) {
    let mut before = storage(f);
    f.engine.abort_callback(KEY, token).unwrap();
    before.arena[..56].copy_from_slice(&state_literal(&frozen(f.esp)));
    before.arena[56..96].copy_from_slice(&exit_literal(8, 7, 17));
    assert_eq!(storage(f), before);
}

#[test]
fn finish_priorities_preserve_full_identity_family_stop_currency_and_authority() {
    let mut f = fixture(0x9000, false);
    let callback = f.callback;
    reject(&mut f, KEY, callback, 0, call(CallError::InvalidToken));
    let outer_token = capture(&mut f, true);
    let record = begin(&mut f, outer_token, &[], false);
    stop(&mut f, 42, 0);
    reject(&mut f, KEY, callback, record.token, call(CallError::Busy));
    f.engine.arena_mut().unwrap()[..96].fill(0xff);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(&mut f, KEY, callback, record.token, call(CallError::Busy));
    stop(&mut f, 42, 0);
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    reject(
        &mut f,
        KEY ^ (1_u64 << 63),
        callback,
        0,
        HostError::InvalidArtifact,
    );
    reject(
        &mut f,
        KEY,
        callback ^ (1_u64 << 63),
        record.token,
        invalid_unit(),
    );
    reject(&mut f, KEY, 0, 0, invalid_unit());
    let outer = f.outer;
    let foreign = f.foreign;
    for (id, token) in [
        (callback, 0),
        (callback, record.token + 1),
        (callback, outer_token),
        (outer, record.token),
        (foreign, record.token),
    ] {
        reject(&mut f, KEY, id, token, call(CallError::InvalidToken));
    }
    abort_restores(&mut f, record.token);
    let record = begin(&mut f, outer_token, &[], true);
    assert_eq!(record.token, 3);
    stop(&mut f, 0x8000_0001, u32::MAX);
    let canonical = f.engine.arena()[..96].to_vec();
    for (offset, value) in [
        (0, 0_u32),
        (4, 0x10002),
        (4, 0x20001),
        (8, 55),
        (12, 1),
        (52, 0),
        (52, 0x8000_0002),
        (56, 0),
        (60, 0x10002),
        (60, 0x20003),
        (64, 39),
        (68, 1),
        (72, 3),
        (72, 1),
        (72, 2),
        (72, 6),
        (80, 0),
        (80, 17),
        (84, 1),
        (88, 1),
        (92, 4),
        (48, ENTRY),
        (48, OUTER),
        (48, RETURN + 1),
        (32, 0x8ffc),
        (32, 0x9004),
    ] {
        f.engine.arena_mut().unwrap()[..96].copy_from_slice(&canonical);
        f.engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(
            &mut f,
            KEY,
            callback,
            record.token,
            call(CallError::InvalidStop),
        );
    }
    for reason in [1_u32, 2, 3, 6] {
        f.engine.arena_mut().unwrap()[..96].copy_from_slice(&canonical);
        f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&exit_literal(reason, 0, 0));
        reject(
            &mut f,
            KEY,
            callback,
            record.token,
            call(CallError::InvalidStop),
        );
    }
    f.engine.arena_mut().unwrap()[..96].copy_from_slice(&canonical);
    reject(
        &mut f,
        KEY,
        callback,
        record.token,
        call(CallError::Cancelled),
    );
    reject(&mut f, KEY, callback, 0, call(CallError::InvalidToken));
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    let before = observe(&f);
    assert_eq!(
        f.engine.finish_callback(KEY, f.generation, record.token),
        Err(call(CallError::InvalidToken))
    );
    assert_eq!(observe(&f), before);
    assert_eq!(
        f.engine
            .finish_resident_callback(KEY, callback, record.token)
            .unwrap()
            .result,
        0x8000_0001
    );

    let mut family = fixture(0x9000, false);
    let outer_token = capture(&mut family, false);
    let replacement = family
        .engine
        .begin_callback(KEY, family.generation, outer_token, ENTRY, RETURN, 18, &[])
        .unwrap();
    stop(&mut family, 42, 0);
    let id = family.callback;
    reject(
        &mut family,
        KEY,
        id,
        replacement.token,
        call(CallError::InvalidToken),
    );
    assert_eq!(
        family
            .engine
            .finish_callback(KEY, family.generation, replacement.token)
            .unwrap()
            .result,
        42
    );

    let mut same = fixture(0x9000, true);
    let t = capture(&mut same, true);
    let r = begin(&mut same, t, &[], false);
    stop(&mut same, 42, 0);
    let id = same.callback;
    reject(&mut same, KEY, id, r.token, call(CallError::Busy));
    same.engine.abort_callback(KEY, r.token).unwrap();

    for target in [ENTRY, OUTER] {
        let mut stale_fixture = fixture(0x9000, false);
        let t = capture(&mut stale_fixture, true);
        let r = begin(&mut stale_fixture, t, &[], true);
        stop(&mut stale_fixture, 42, 0);
        let value = read_word(&stale_fixture, target);
        stale_fixture.engine.write32(target, value).unwrap();
        stale_fixture.engine.arena_mut().unwrap()[..96].fill(0xff);
        stale_fixture.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        let id = stale_fixture.callback;
        reject(&mut stale_fixture, KEY, id, r.token, stale());
        reject(
            &mut stale_fixture,
            KEY,
            id,
            0,
            if target == ENTRY {
                stale()
            } else {
                call(CallError::InvalidToken)
            },
        );
        abort_restores(&mut stale_fixture, r.token);
    }
    let mut closed = fixture(0x9000, false);
    let t = capture(&mut closed, true);
    let r = begin(&mut closed, t, &[], true);
    stop(&mut closed, 42, 0);
    closed.engine.close();
    let id = closed.callback;
    reject(&mut closed, 0, 0, 0, HostError::Closed);
    reject(&mut closed, KEY, id, r.token, HostError::Closed);
}

#[test]
fn finish_consumes_once_restores_owner_and_allows_fresh_unarmed_callbacks() {
    for (esp, args, value, retired) in [
        (0x9000, 0, 0, 0),
        (0x9000, 2, u32::MAX, u32::MAX),
        (0, 16, 0x8000_0001, 0),
    ] {
        let mut f = fixture(esp, false);
        let installation = observe(&f).installed;
        let outer_token = capture(&mut f, true);
        let arguments = (0..args).map(|n| 0x1122_0000 + n).collect::<Vec<_>>();
        let record = begin(&mut f, outer_token, &arguments, true);
        assert_eq!(record.entry_esp, esp.wrapping_sub(4 * (args + 1)));
        stop(&mut f, value, retired);
        f.engine.arena_mut().unwrap()[100..140].fill(0x5a);
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0xa5);
        f.engine.write32(esp.wrapping_sub(4), 0xffff_ffff).unwrap();
        let mut before = storage(&f);
        let result = f
            .engine
            .finish_resident_callback(KEY, f.callback, record.token)
            .unwrap();
        assert_eq!(
            (
                result.token,
                result.outer_token,
                result.result,
                result.outer_unit_id,
                result.callback_unit_id
            ),
            (2, 1, value, f.outer, f.callback)
        );
        before.arena[..56].copy_from_slice(&state_literal(&frozen(esp)));
        before.arena[56..96].copy_from_slice(&exit_literal(8, 7, 17));
        before.arena[140..188].copy_from_slice(&result_literal(&f, 2, 1, value));
        assert_eq!(storage(&f), before);
        assert_eq!(observe(&f).installed, installation);
        let id = f.callback;
        reject(&mut f, KEY, id, 2, call(CallError::InvalidToken));
        let unchanged = observe(&f);
        assert_eq!(
            f.engine.abort_callback(KEY, 2),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(
            f.engine.complete_resident_call(KEY, f.foreign, 1, value),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(
            f.engine.complete_call(KEY, f.generation, 1, value),
            Err(call(CallError::InvalidToken))
        );
        assert_eq!(observe(&f), unchanged);
        assert_eq!(
            f.engine.guard_dispatch_entry(KEY),
            Err(call(CallError::Busy))
        );
        assert_eq!(
            f.engine.guard(KEY, f.generation),
            Err(call(CallError::Busy))
        );
        let next = begin(&mut f, 1, &[], false);
        assert_eq!(next.token, 3);
        stop(&mut f, value, 0);
        reject(&mut f, KEY, id, 2, call(CallError::InvalidToken));
        reject(&mut f, KEY, id, 3, call(CallError::Busy));
        abort_restores(&mut f, 3);
        let before = storage(&f);
        f.engine
            .complete_resident_call(KEY, f.outer, 1, value)
            .unwrap();
        let mut expected = before;
        let mut completed = frozen(esp);
        completed.registers[0] = value;
        completed.registers[4] = esp.wrapping_add(4);
        completed.eip = CALLER;
        expected.arena[..56].copy_from_slice(&state_literal(&completed));
        expected.arena[56..96].copy_from_slice(&exit_literal(3, 0, 0));
        assert_eq!(storage(&f), expected);
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    }
}

#[test]
fn finish_preserves_committed_smc_and_abort_recovery_without_stack_rereads() {
    for target in [ENTRY, OUTER] {
        let mut f = fixture(0x9000, false);
        let t = capture(&mut f, true);
        let r = begin(&mut f, t, &[], true);
        let value = read_word(&f, target);
        assert_eq!(
            f.engine.store_resident32(KEY, f.callback, target, value),
            Ok(if target == ENTRY {
                StoreCompletion::CodeInvalidated
            } else {
                StoreCompletion::Complete
            })
        );
        stop(&mut f, 42, 0);
        let id = f.callback;
        reject(&mut f, KEY, id, r.token, stale());
        reject(
            &mut f,
            KEY,
            id,
            0,
            if target == ENTRY {
                stale()
            } else {
                call(CallError::InvalidToken)
            },
        );
        f.engine.arena_mut().unwrap()[..96].fill(0xff);
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        abort_restores(&mut f, r.token);
        f.engine.arena_mut().unwrap()[96..100].fill(0);
        let before = storage(&f);
        if target == OUTER {
            assert_eq!(
                f.engine.complete_resident_call(KEY, f.outer, t, 42),
                Err(stale())
            );
            assert_eq!(storage(&f), before);
            f.engine.abandon_call(KEY, t).unwrap();
        } else {
            f.engine
                .complete_resident_call(KEY, f.outer, t, 42)
                .unwrap();
        }
        assert_eq!(read_word(&f, target), value);
        assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    }
    let mut f = fixture(0x9000, false);
    let t = capture(&mut f, true);
    let r = begin(&mut f, t, &[0x1122_3344, 0x5566_7788], true);
    stop(&mut f, 42, 0);
    f.engine.unmap(0x8000, 1).unwrap();
    f.engine.unmap(0x9000, 1).unwrap();
    let mut expected = storage(&f);
    assert_eq!(
        f.engine
            .finish_resident_callback(KEY, f.callback, r.token)
            .unwrap()
            .result,
        42
    );
    expected.arena[..56].copy_from_slice(&state_literal(&frozen(f.esp)));
    expected.arena[56..96].copy_from_slice(&exit_literal(8, 7, 17));
    expected.arena[140..188].copy_from_slice(&result_literal(&f, r.token, t, 42));
    assert_eq!(storage(&f), expected);
    f.engine
        .complete_resident_call(KEY, f.outer, t, 42)
        .unwrap();
    assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
}
