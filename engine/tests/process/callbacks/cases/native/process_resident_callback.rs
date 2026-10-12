use ring3_engine::{
    abi::{
        resident_callback::ResidentCallbackRecord32,
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const GATE: u32 = 0x1000;
const ENTRY: u32 = 0x2000;
const CALLBACK_RETURN: u32 = 0x2100;
const CALLER_RETURN: u32 = 0x3000;
const GATE_ID: u32 = 17;
const RETURN_ID: u32 = 18;

struct Fixture {
    engine: EngineInstance,
    outer_id: u64,
    callback_id: u64,
    generation: u32,
    esp: u32,
    pages: Vec<u32>,
}

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn descriptors(engine: &mut EngineInstance, values: &[(u32, u32)]) {
    let arena = engine.arena_mut().unwrap();
    for (index, (pc, value)) in values.iter().enumerate() {
        arena[140 + index * 8..144 + index * 8].copy_from_slice(&pc.to_le_bytes());
        arena[144 + index * 8..148 + index * 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture(esp: u32, same_unit: bool) -> Fixture {
    configured_fixture(esp, same_unit, false)
}

fn configured_fixture(esp: u32, same_unit: bool, wrap: bool) -> Fixture {
    let mut pages = vec![GATE, ENTRY, CALLER_RETURN, 0x8000, 0x9000];
    if wrap {
        pages.extend([0, 0xffff_f000]);
    }
    let mut engine = EngineInstance::new(pages.len() as u32, KEY).unwrap();
    for &page in &pages {
        engine.map(page, 1, 7).unwrap();
    }
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90]);
    upload(&mut engine, CALLBACK_RETURN, &[0x0f, 0x0b]);
    upload(&mut engine, CALLER_RETURN, &[0x90]);
    for offset in 1..=17 {
        engine
            .write32(esp.wrapping_sub(offset * 4), 0xa0b0_c000 + offset)
            .unwrap();
    }
    engine.write32(esp, CALLER_RETURN).unwrap();
    // code-page stack controls must retain the authored gate bytes before compilation.
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, ENTRY, &[0x90]);
    upload(&mut engine, CALLBACK_RETURN, &[0x0f, 0x0b]);
    let all = [
        (GATE, 2),
        (ENTRY, 1),
        (CALLBACK_RETURN, 2),
        (GATE, GATE_ID),
        (CALLBACK_RETURN, RETURN_ID),
    ];
    descriptors(&mut engine, &all);
    let generation = engine.compile_with_gates(3, 2).unwrap();
    assert_eq!(generation, 1);
    let (outer_id, callback_id) = if same_unit {
        descriptors(&mut engine, &all);
        let id = engine.compile_resident_with_gates(3, 2).unwrap().get();
        (id, id)
    } else {
        descriptors(&mut engine, &[(GATE, 2), (GATE, GATE_ID)]);
        let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
        descriptors(
            &mut engine,
            &[
                (ENTRY, 1),
                (CALLBACK_RETURN, 2),
                (CALLBACK_RETURN, RETURN_ID),
            ],
        );
        let callback = engine.compile_resident_with_gates(2, 1).unwrap().get();
        (outer, callback)
    };
    engine
        .acknowledge_resident_installation(KEY, outer_id, 0)
        .unwrap();
    if callback_id != outer_id {
        engine
            .acknowledge_resident_installation(KEY, callback_id, 1)
            .unwrap();
    }
    Fixture {
        engine,
        outer_id,
        callback_id,
        generation,
        esp,
        pages,
    }
}

fn frozen_state(esp: u32) -> State32 {
    State32 {
        registers: [
            0x1122_3344,
            0x5566_7788,
            0x99aa_bbcc,
            0xddee_ff00,
            esp,
            0x1234_5678,
            0x2345_6789,
            0x3456_789a,
        ],
        eip: GATE,
        eflags: 0xcd7,
    }
}

fn capture(f: &mut Fixture, resident: bool) -> u32 {
    let arena = f.engine.arena_mut().unwrap();
    // typed native Gate controls; the actual-Wasm proof supplies translated stops.
    encode_state(&frozen_state(f.esp), &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: GATE_ID },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    if resident {
        f.engine
            .capture_resident_call(KEY, f.outer_id, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    } else {
        f.engine
            .capture_call(KEY, f.generation, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token
    }
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
fn record_literal(f: &Fixture, token: u32, outer: u32, count: u32, outcome: u32) -> Vec<u8> {
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
        f.esp.wrapping_sub(4 * (count + 1)),
        CALLBACK_RETURN,
        RETURN_ID,
        count,
        0,
        f.outer_id as u32,
        (f.outer_id >> 32) as u32,
        f.callback_id as u32,
        (f.callback_id >> 32) as u32,
    ])
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    arena: Vec<u8>,
    arena_address: usize,
    generation: u32,
    modules: Vec<ModuleObservation>,
    guards: Vec<Result<(), HostError>>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}
type ModuleObservation = Result<(Vec<u8>, usize), HostError>;
fn observe(f: &Fixture) -> Observation {
    let copy = |b: &[u8]| (b.to_vec(), b.as_ptr() as usize);
    Observation {
        arena: f.engine.arena().to_vec(),
        arena_address: f.engine.arena_address(),
        generation: f.engine.generation(),
        modules: vec![
            f.engine.dispatcher_bytes(KEY).map(copy),
            f.engine.artifact_bytes().map(copy),
            f.engine.resident_bytes(f.outer_id).map(copy),
            f.engine.resident_bytes(f.callback_id).map(copy),
        ],
        guards: vec![
            f.engine.guard_dispatch_entry(KEY),
            f.engine.guard(KEY, f.generation),
            f.engine.guard_resident(KEY, f.outer_id),
            f.engine.guard_resident(KEY, f.callback_id),
        ],
        installed: [GATE, ENTRY, CALLBACK_RETURN, CALLER_RETURN]
            .map(|pc| f.engine.lookup_installed_resident(KEY, pc))
            .to_vec(),
        ram: f
            .pages
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
    }
}
fn read_word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[derive(Clone)]
struct Request {
    key: u64,
    outer_id: u64,
    callback_id: u64,
    outer_token: u32,
    entry_pc: u32,
    return_pc: u32,
    return_id: u32,
    arguments: Vec<u32>,
}
type RequestMutation = (fn(&mut Request), HostError);
fn request(f: &Fixture, outer_token: u32) -> Request {
    Request {
        key: KEY,
        outer_id: f.outer_id,
        callback_id: f.callback_id,
        outer_token,
        entry_pc: ENTRY,
        return_pc: CALLBACK_RETURN,
        return_id: RETURN_ID,
        arguments: vec![],
    }
}
fn begin(f: &mut Fixture, r: &Request) -> Result<ResidentCallbackRecord32, HostError> {
    f.engine.begin_resident_callback(
        r.key,
        r.outer_id,
        r.callback_id,
        r.outer_token,
        r.entry_pc,
        r.return_pc,
        r.return_id,
        &r.arguments,
    )
}
fn reject(f: &mut Fixture, r: &Request, error: HostError) {
    let before = observe(f);
    assert_eq!(begin(f, r), Err(error));
    assert_eq!(observe(f), before);
}
fn call_error(error: CallError) -> HostError {
    HostError::Call(error)
}
fn ram_after_frame(
    f: &Fixture,
    before: &Observation,
    arguments: &[u32],
) -> Vec<Result<Vec<u8>, HostError>> {
    let mut ram = before.ram.clone();
    let start = f.esp.wrapping_sub(4 * (arguments.len() as u32 + 1));
    for (index, value) in std::iter::once(CALLBACK_RETURN)
        .chain(arguments.iter().copied())
        .enumerate()
    {
        let at = start.wrapping_add(index as u32 * 4);
        let page = f
            .pages
            .iter()
            .position(|&page| page == (at & !0xfff))
            .unwrap();
        let offset = (at & 0xfff) as usize;
        ram[page].as_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    ram
}

#[test]
fn resident_callback_admission_preserves_exact_outer_owner_and_literal_outputs() {
    let mut f = fixture(0x9000, false);
    assert_eq!(f.engine.guard(KEY, 1), Ok(()));
    let outer = capture(&mut f, true);
    assert_eq!(outer, 1);
    let before = observe(&f);
    let record = f
        .engine
        .begin_resident_callback(
            KEY,
            f.outer_id,
            f.callback_id,
            outer,
            ENTRY,
            CALLBACK_RETURN,
            RETURN_ID,
            &[0x1122_3344, 0x5566_7788],
        )
        .unwrap();
    assert_eq!(
        record,
        ResidentCallbackRecord32 {
            token: 2,
            outer_token: 1,
            phase: 1,
            outcome: 0,
            entry_pc: ENTRY,
            entry_esp: 0x8ff4,
            return_pc: CALLBACK_RETURN,
            return_id: RETURN_ID,
            stack_words: 2,
            result: 0,
            outer_unit_id: f.outer_id,
            callback_unit_id: f.callback_id
        }
    );
    let mut wanted = before.arena.clone();
    let mut entered = frozen_state(f.esp);
    entered.eip = ENTRY;
    entered.registers[4] = 0x8ff4;
    wanted[..56].copy_from_slice(&state_literal(&entered));
    wanted[56..96].copy_from_slice(&exit_literal(3, 0, 0));
    wanted[140..212].copy_from_slice(&record_literal(&f, 2, 1, 2, 0));
    assert_eq!(f.engine.arena(), wanted);
    let after = observe(&f);
    assert_eq!(after.modules, before.modules);
    assert_eq!(after.installed, before.installed);
    assert_eq!(after.arena_address, before.arena_address);
    assert_eq!(after.generation, before.generation);
    let busy = Err(HostError::Call(CallError::Busy));
    assert_eq!(after.guards, vec![busy; 4]);
    let mut expected_ram = before.ram.clone();
    let page = expected_ram[3].as_mut().unwrap();
    page[0xff4..0x1000].copy_from_slice(&words(&[CALLBACK_RETURN, 0x1122_3344, 0x5566_7788]));
    assert_eq!(after.ram, expected_ram);
    assert_eq!(read_word(&f.engine, f.esp), CALLER_RETURN);
}

#[test]
fn resident_admission_priorities_and_both_owner_families_reject_without_publication() {
    let mut f = fixture(0x9000, false);
    let mut r = request(&f, 1);
    reject(&mut f, &r, call_error(CallError::InvalidToken));
    r.arguments = vec![0; 17];
    reject(&mut f, &r, call_error(CallError::InvalidRequest));
    let outer = capture(&mut f, true);
    let r = request(&f, outer);
    let before = observe(&f);
    assert_eq!(
        f.engine.begin_callback(
            KEY,
            f.generation,
            outer,
            ENTRY,
            CALLBACK_RETURN,
            RETURN_ID,
            &[]
        ),
        Err(call_error(CallError::InvalidToken))
    );
    assert_eq!(observe(&f), before);
    let mutations: [RequestMutation; 13] = [
        (|r| r.key ^= 1_u64 << 32, HostError::InvalidArtifact),
        (|r| r.key = r.key as u32 as u64, HostError::InvalidArtifact),
        (
            |r| r.outer_id ^= 1_u64 << 32,
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
        ),
        (
            |r| r.callback_id ^= 1_u64 << 32,
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
        ),
        (
            |r| r.outer_id = 0,
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
        ),
        (
            |r| r.callback_id = 0,
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
        ),
        (|r| r.outer_token = 0, call_error(CallError::InvalidToken)),
        (|r| r.outer_token += 1, call_error(CallError::InvalidToken)),
        (
            |r| r.arguments = vec![0; 17],
            call_error(CallError::InvalidRequest),
        ),
        (|r| r.entry_pc += 1, call_error(CallError::InvalidRequest)),
        (
            |r| r.entry_pc = CALLBACK_RETURN,
            call_error(CallError::InvalidRequest),
        ),
        (|r| r.return_id = 0, call_error(CallError::InvalidRequest)),
        (
            |r| r.return_id = GATE_ID,
            call_error(CallError::InvalidRequest),
        ),
    ];
    for (mutate, error) in mutations {
        let mut bad = r.clone();
        mutate(&mut bad);
        reject(&mut f, &bad, error);
    }
    let mut bad = r.clone();
    bad.outer_id = f.callback_id;
    bad.callback_id = f.outer_id;
    reject(&mut f, &bad, call_error(CallError::InvalidToken));
    let mut bad = r.clone();
    bad.callback_id = f.outer_id;
    reject(&mut f, &bad, call_error(CallError::InvalidRequest));
    let mut bad = r.clone();
    bad.return_pc += 1;
    reject(&mut f, &bad, call_error(CallError::InvalidRequest));
    let saved = f.engine.arena()[..100].to_vec();
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    let mut malformed_request = r.clone();
    malformed_request.entry_pc = u32::MAX;
    reject(&mut f, &malformed_request, call_error(CallError::Cancelled));
    f.engine.arena_mut().unwrap()[0] = 0;
    reject(
        &mut f,
        &malformed_request,
        call_error(CallError::StateChanged),
    );
    let mut invalid_identity = malformed_request.clone();
    invalid_identity.callback_id ^= 1_u64 << 32;
    invalid_identity.arguments = vec![0; 17];
    reject(
        &mut f,
        &invalid_identity,
        HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
    );
    let mut invalid_count = malformed_request.clone();
    invalid_count.arguments = vec![0; 17];
    reject(
        &mut f,
        &invalid_count,
        call_error(CallError::InvalidRequest),
    );
    f.engine.arena_mut().unwrap()[..100].copy_from_slice(&saved);
    f.engine.arena_mut().unwrap()[76..80].copy_from_slice(&4_u32.to_le_bytes());
    reject(&mut f, &r, call_error(CallError::StateChanged));
    f.engine.arena_mut().unwrap()[..100].copy_from_slice(&saved);
    let record = begin(&mut f, &r).unwrap();
    assert_eq!(record.token, 2);
    f.engine.abort_callback(KEY, record.token).unwrap();
    f.engine
        .complete_resident_call(KEY, f.outer_id, outer, 0xaabb_ccdd)
        .unwrap();
    let replacement_outer = capture(&mut f, false);
    assert_eq!(replacement_outer, 3);
    let replacement_request = request(&f, replacement_outer);
    reject(
        &mut f,
        &replacement_request,
        call_error(CallError::InvalidToken),
    );
    f.engine.abandon_call(KEY, replacement_outer).unwrap();
    for stale_pc in [GATE, ENTRY] {
        let mut stale_fixture = fixture(0x9000, false);
        let stale_outer = capture(&mut stale_fixture, true);
        let same_bytes = read_word(&stale_fixture.engine, stale_pc);
        stale_fixture.engine.write32(stale_pc, same_bytes).unwrap();
        stale_fixture.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        let mut stale_request = request(&stale_fixture, stale_outer);
        stale_request.arguments = vec![0; 17];
        reject(
            &mut stale_fixture,
            &stale_request,
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::CodeInvalidated),
        );
    }
}

#[test]
fn transfer_arguments_are_bounded_copied_and_wrap_as_seventeen_atomic_words() {
    let mut f = configured_fixture(0x20, false, true);
    let outer = capture(&mut f, true);
    f.engine.arena_mut().unwrap()[140..212].fill(0xa5);
    for count in [17, u32::MAX] {
        let before = observe(&f);
        assert_eq!(
            f.engine.begin_resident_callback_from_transfer(
                KEY,
                f.outer_id,
                f.callback_id,
                outer,
                ENTRY,
                CALLBACK_RETURN,
                RETURN_ID,
                count
            ),
            Err(call_error(CallError::InvalidRequest))
        );
        assert_eq!(observe(&f), before);
    }
    let arguments: Vec<u32> = (0..16).map(|index| 0x1000_0000 + index * 0x10101).collect();
    f.engine.arena_mut().unwrap()[140..204].copy_from_slice(&words(&arguments));
    let before = observe(&f);
    let record = f
        .engine
        .begin_resident_callback_from_transfer(
            KEY,
            f.outer_id,
            f.callback_id,
            outer,
            ENTRY,
            CALLBACK_RETURN,
            RETURN_ID,
            16,
        )
        .unwrap();
    assert_eq!(
        (
            record.token,
            record.outer_token,
            record.entry_esp,
            record.stack_words,
            record.outcome
        ),
        (2, 1, 0xffff_ffdc, 16, 0)
    );
    let mut wanted = before.arena.clone();
    let mut entered = frozen_state(f.esp);
    entered.eip = ENTRY;
    entered.registers[4] = 0xffff_ffdc;
    wanted[..56].copy_from_slice(&state_literal(&entered));
    wanted[56..96].copy_from_slice(&exit_literal(3, 0, 0));
    wanted[140..212].copy_from_slice(&record_literal(&f, 2, 1, 16, 0));
    assert_eq!(f.engine.arena(), wanted);
    assert_eq!(observe(&f).ram, ram_after_frame(&f, &before, &arguments));
    assert_eq!(read_word(&f.engine, 0xffff_ffdc), CALLBACK_RETURN);
    assert_eq!(read_word(&f.engine, 0xffff_fffc), arguments[7]);
    assert_eq!(read_word(&f.engine, 0), arguments[8]);
    assert_eq!(read_word(&f.engine, 0x1c), arguments[15]);
    assert_eq!(read_word(&f.engine, 0x20), CALLER_RETURN);
    f.engine.arena_mut().unwrap()[140..212].fill(0x5a);
    let before_abort = observe(&f);
    f.engine.abort_callback(KEY, record.token).unwrap();
    let mut restored = before_abort.arena.clone();
    restored[..56].copy_from_slice(&state_literal(&frozen_state(f.esp)));
    restored[56..96].copy_from_slice(&exit_literal(8, 3, GATE_ID));
    assert_eq!(f.engine.arena(), restored);
    assert_eq!(observe(&f).ram, before_abort.ram);
    f.engine
        .complete_resident_call(KEY, f.outer_id, outer, 55)
        .unwrap();
    let mut completed = frozen_state(f.esp);
    completed.registers[0] = 55;
    completed.registers[4] = 0x24;
    completed.eip = CALLER_RETURN;
    assert_eq!(&f.engine.arena()[..56], state_literal(&completed));
    assert_eq!(&f.engine.arena()[56..96], exit_literal(3, 0, 0));
}

#[test]
fn late_multiword_fault_preserves_ram_snapshots_and_the_unconsumed_callback_token() {
    let mut f = fixture(0x9004, false);
    let outer = capture(&mut f, true);
    f.engine.protect(0x9000, 1, 1).unwrap();
    let snapshots: Vec<_> = [GATE, ENTRY, CALLER_RETURN, 0x8ff8]
        .map(|pc| {
            f.engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(pc), 1)
                .unwrap()
        })
        .into_iter()
        .collect();
    let mut r = request(&f, outer);
    r.arguments = vec![0x1122_3344, 0x5566_7788];
    reject(
        &mut f,
        &r,
        call_error(CallError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x9000),
            access: Access::Write,
            reason: FaultReason::Permission,
        }))),
    );
    assert!(
        snapshots
            .iter()
            .all(|snapshot| f.engine.memory().unwrap().is_code_current(snapshot))
    );
    f.engine.protect(0x9000, 1, 7).unwrap();
    let before = observe(&f);
    let record = begin(&mut f, &r).unwrap();
    assert_eq!(record.token, 2);
    assert_eq!(record.outcome, 0);
    assert_eq!(observe(&f).ram, ram_after_frame(&f, &before, &r.arguments));
    f.engine.abort_callback(KEY, record.token).unwrap();
    f.engine
        .complete_resident_call(KEY, f.outer_id, outer, 55)
        .unwrap();
}

#[test]
fn committed_snapshot_outcomes_and_same_unit_aliases_always_allow_frozen_abort() {
    let stale = HostError::Resident(ring3_engine::cpu::dbt::RegistryError::CodeInvalidated);
    for (esp, same_unit, arguments, outcome) in [
        (0x9000, false, vec![], 0),
        (0x2104, false, vec![], 1),
        (0x1104, false, vec![], 2),
        (0x2004, false, vec![0x1122_3344, 0x5566_7788], 3),
        (0x2104, true, vec![], 3),
    ] {
        let mut f = fixture(esp, same_unit);
        let outer = capture(&mut f, true);
        let before = observe(&f);
        let mut r = request(&f, outer);
        r.arguments = arguments;
        let record = begin(&mut f, &r).unwrap();
        assert_eq!(
            (record.token, record.outer_token, record.outcome),
            (2, 1, outcome)
        );
        let mut wanted = before.arena.clone();
        let mut entered = frozen_state(f.esp);
        entered.eip = ENTRY;
        entered.registers[4] = esp.wrapping_sub(4 * (r.arguments.len() as u32 + 1));
        wanted[..56].copy_from_slice(&state_literal(&entered));
        wanted[56..96].copy_from_slice(&exit_literal(3, 0, 0));
        wanted[140..212].copy_from_slice(&record_literal(
            &f,
            2,
            1,
            r.arguments.len() as u32,
            outcome,
        ));
        assert_eq!(f.engine.arena(), wanted);
        let after = observe(&f);
        assert_eq!(after.ram, ram_after_frame(&f, &before, &r.arguments));
        assert_eq!(after.modules[0], before.modules[0]);
        if outcome & 2 == 0 {
            assert_eq!(after.modules[2], before.modules[2]);
            assert_eq!(after.guards[2], Err(call_error(CallError::Busy)));
        } else {
            assert_eq!(after.modules[2], Err(stale));
            assert_eq!(after.guards[2], Err(stale));
        }
        if outcome & 1 == 0 {
            assert_eq!(after.modules[3], before.modules[3]);
            assert_eq!(after.guards[3], Err(call_error(CallError::Busy)));
        } else {
            assert_eq!(after.modules[3], Err(stale));
            assert_eq!(after.guards[3], Err(stale));
        }
        assert_eq!(after.guards[0], Err(call_error(CallError::Busy)));
        assert_eq!(
            after.guards[1],
            Err(if outcome == 0 {
                call_error(CallError::Busy)
            } else {
                HostError::CodeInvalidated
            })
        );
        f.engine.arena_mut().unwrap()[..96].fill(0xff);
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        f.engine.arena_mut().unwrap()[140..212].fill(0x5a);
        let before_abort = observe(&f);
        f.engine.abort_callback(KEY, record.token).unwrap();
        let mut restored = before_abort.arena.clone();
        restored[..56].copy_from_slice(&state_literal(&frozen_state(f.esp)));
        restored[56..96].copy_from_slice(&exit_literal(8, 3, GATE_ID));
        assert_eq!(f.engine.arena(), restored);
        assert_eq!(observe(&f).ram, before_abort.ram);
        if outcome & 2 == 0 {
            f.engine.arena_mut().unwrap()[96..100].fill(0);
            f.engine
                .complete_resident_call(KEY, f.outer_id, outer, 55)
                .unwrap();
            let mut completed = frozen_state(f.esp);
            completed.registers[0] = 55;
            completed.registers[4] = esp.wrapping_add(4);
            completed.eip = CALLER_RETURN;
            assert_eq!(&f.engine.arena()[..56], state_literal(&completed));
            assert_eq!(&f.engine.arena()[56..96], exit_literal(3, 0, 0));
        } else {
            let before_completion = observe(&f);
            assert_eq!(
                f.engine.complete_resident_call(KEY, f.outer_id, outer, 55),
                Err(stale)
            );
            assert_eq!(observe(&f), before_completion);
            f.engine.abandon_call(KEY, outer).unwrap();
            assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
        }
    }
}

#[test]
fn suspended_family_blocks_legacy_execution_and_migration_until_exact_token_revocation() {
    let mut f = fixture(0x9000, false);
    let outer = capture(&mut f, true);
    let r = request(&f, outer);
    let record = begin(&mut f, &r).unwrap();
    let mut bad = r.clone();
    bad.outer_token = 0;
    bad.arguments = vec![0; 17];
    bad.entry_pc = u32::MAX;
    reject(&mut f, &bad, call_error(CallError::Busy));
    bad.outer_id ^= 1_u64 << 32;
    reject(
        &mut f,
        &bad,
        HostError::Resident(ring3_engine::cpu::dbt::RegistryError::InvalidUnit),
    );
    let mut wrong_key = r.clone();
    wrong_key.key ^= 1_u64 << 32;
    reject(&mut f, &wrong_key, HostError::InvalidArtifact);
    let before = observe(&f);
    assert_eq!(
        f.engine.compile_with_gates(0, 0),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(
        f.engine.compile_resident_with_gates(0, 0),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(
        f.engine
            .acknowledge_resident_installation(KEY, f.outer_id, 0),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(observe(&f), before);
    f.engine.arena_mut().unwrap()[..56].copy_from_slice(&state_literal(&frozen_state(f.esp)));
    f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&exit_literal(8, 3, GATE_ID));
    let before = observe(&f);
    assert_eq!(
        f.engine.complete_resident_call(KEY, f.outer_id, outer, 55),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(
        f.engine.abandon_call(KEY, outer),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(observe(&f), before);
    f.engine.arena_mut().unwrap()[..96].fill(0xff);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    f.engine.arena_mut().unwrap()[140..212].fill(0x5a);
    let before = observe(&f);
    assert_eq!(
        f.engine.finish_callback(KEY, f.generation, record.token),
        Err(call_error(CallError::InvalidToken))
    );
    assert_eq!(
        f.engine
            .resume_callback_code(KEY, f.generation, record.token, 0, 0),
        Err(call_error(CallError::InvalidToken))
    );
    assert_eq!(
        f.engine
            .resume_callback_entries(KEY, f.generation, record.token, 0, 0),
        Err(call_error(CallError::InvalidToken))
    );
    assert_eq!(
        f.engine.guard(KEY, f.generation),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(
        f.engine.begin_callback(
            KEY,
            f.generation,
            outer,
            ENTRY,
            CALLBACK_RETURN,
            RETURN_ID,
            &[0; 17]
        ),
        Err(call_error(CallError::Busy))
    );
    assert_eq!(
        f.engine.dispatcher_bytes(KEY).unwrap(),
        before.modules[0].as_ref().unwrap().0.as_slice()
    );
    assert_eq!(
        f.engine.resident_bytes(f.outer_id).unwrap(),
        before.modules[2].as_ref().unwrap().0.as_slice()
    );
    assert_eq!(observe(&f), before);
    for (key, token, expected) in [
        (
            KEY ^ (1_u64 << 32),
            record.token,
            HostError::InvalidArtifact,
        ),
        (KEY, 0, call_error(CallError::InvalidToken)),
        (KEY, outer, call_error(CallError::InvalidToken)),
        (KEY, u32::MAX, call_error(CallError::InvalidToken)),
    ] {
        let before = observe(&f);
        assert_eq!(f.engine.abort_callback(key, token), Err(expected));
        assert_eq!(observe(&f), before);
    }
    let before_abort = observe(&f);
    f.engine.abort_callback(KEY, record.token).unwrap();
    let mut restored = before_abort.arena.clone();
    restored[..56].copy_from_slice(&state_literal(&frozen_state(f.esp)));
    restored[56..96].copy_from_slice(&exit_literal(8, 3, GATE_ID));
    assert_eq!(f.engine.arena(), restored);
    assert_eq!(observe(&f).ram, before_abort.ram);
    let before = observe(&f);
    assert_eq!(
        f.engine.complete_resident_call(KEY, f.outer_id, outer, 55),
        Err(call_error(CallError::Cancelled))
    );
    assert_eq!(observe(&f), before);
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    let next = begin(&mut f, &r).unwrap();
    assert_eq!(next.token, 3);
    let before = observe(&f);
    assert_eq!(
        f.engine.abort_callback(KEY, record.token),
        Err(call_error(CallError::InvalidToken))
    );
    assert_eq!(observe(&f), before);
    f.engine.abort_callback(KEY, next.token).unwrap();
    f.engine.abandon_call(KEY, outer).unwrap();
    assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    let final_outer = capture(&mut f, true);
    assert_eq!(final_outer, 4);
    let final_request = request(&f, final_outer);
    let final_callback = begin(&mut f, &final_request).unwrap();
    assert_eq!(final_callback.token, 5);
    let before_close = f.engine.arena().to_vec();
    f.engine.close();
    assert_eq!(f.engine.arena(), before_close);
    let mut closed = final_request;
    closed.key = 0;
    closed.outer_id = 0;
    closed.callback_id = 0;
    closed.outer_token = 0;
    closed.arguments = vec![0; 17];
    reject(&mut f, &closed, HostError::Closed);
    let before = observe(&f);
    assert_eq!(f.engine.abort_callback(0, 0), Err(HostError::Closed));
    assert_eq!(f.engine.finish_callback(0, 0, 0), Err(HostError::Closed));
    assert_eq!(observe(&f), before);
}
