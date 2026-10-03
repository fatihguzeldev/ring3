use ring3_engine::{
    abi::{
        call_frame::CallRecord32,
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const PAGES: [u32; 5] = [0x3000, 0x4000, 0x5000, 0x6000, 0x8000];

fn words(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn descriptors(engine: &mut EngineInstance, pairs: &[(u32, u32)]) {
    for (index, (pc, value)) in pairs.iter().enumerate() {
        let start = 140 + index * 8;
        let arena = engine.arena_mut().unwrap();
        arena[start..start + 4].copy_from_slice(&pc.to_le_bytes());
        arena[start + 4..start + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn ram(engine: &EngineInstance) -> [Vec<u8>; 5] {
    PAGES.map(|pc| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(pc), &mut bytes)
            .unwrap();
        bytes
    })
}

#[test]
fn active_unit_inner_capture_publishes_a_frozen_frame_without_rebinding_home() {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for pc in PAGES {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (0x4000, &[0x0f, 0x0b][..]),
        (0x5000, &[0x90][..]),
        (0x5100, &[0x0f, 0x0b][..]),
        (0x6000, &[0x90][..]),
        (0x6008, &[0x90][..]),
        (0x6200, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.write32(0x8ffc, 0x3005).unwrap();
    descriptors(&mut engine, &[(0x4000, 2), (0x4000, 17)]);
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    descriptors(&mut engine, &[(0x5000, 1), (0x5100, 2), (0x5100, 18)]);
    let home = engine.compile_resident_with_gates(2, 1).unwrap().get();
    descriptors(
        &mut engine,
        &[(0x6000, 1), (0x6008, 1), (0x6200, 2), (0x6200, 19)],
    );
    let active = engine.compile_resident_with_gates(3, 1).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, active, 2)
        .unwrap();
    let outer_state = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: 0x4000,
        eflags: 0xcd7,
    };
    // typed native stops and stack controls only; no guest instructions execute.
    encode_state(&outer_state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let frozen_outer = engine.arena()[..96].to_vec();
    assert_eq!(
        engine
            .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        1
    );
    let admitted = engine
        .begin_resident_callback(KEY, outer, home, 1, 0x5000, 0x5100, 18, &[])
        .unwrap();
    assert_eq!(
        (admitted.token, admitted.outer_token, admitted.entry_esp),
        (2, 1, 0x8ff8)
    );
    let receipt = words(&[
        u32::from_le_bytes(*b"R3RC"),
        0x10001,
        72,
        0,
        2,
        1,
        1,
        0,
        0x5000,
        0x8ff8,
        0x5100,
        18,
        0,
        0,
        outer as u32,
        (outer >> 32) as u32,
        home as u32,
        (home >> 32) as u32,
    ]);
    assert_eq!(&engine.arena()[140..212], receipt);
    engine.authorize_resident_callback(KEY, home, 2).unwrap();
    let mut stopped = outer_state;
    stopped.eip = 0x6000;
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
    let before_selection = engine.arena().to_vec();
    engine
        .select_resident_callback_unit(KEY, home, 2, active)
        .unwrap();
    assert_eq!(engine.arena(), before_selection);
    assert_eq!(engine.guard_resident(KEY, active), Ok(()));
    engine.write32(0x8ff4, 0x6008).unwrap();
    stopped.eip = 0x6200;
    stopped.registers[0] = 18;
    stopped.registers[4] = 0x8ff4;
    encode_state(&stopped, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 2,
            reason: ExitReason::Gate { id: 19 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    engine.arena_mut().unwrap()[212..].fill(0x6d);
    let before = engine.arena().to_vec();
    let before_ram = ram(&engine);
    let before_versions = PAGES.map(|pc| {
        format!(
            "{:?}",
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(pc), 4096)
        )
    });
    let modules = [outer, home, active].map(|id| {
        let bytes = engine.resident_bytes(id).unwrap();
        (id, bytes.to_vec(), bytes.as_ptr() as usize)
    });
    let expected_record = CallRecord32 {
        token: 3,
        id: 19,
        convention: 1,
        stack_words: 0,
        gate_pc: 0x6200,
        entry_esp: 0x8ff4,
        return_pc: 0x6008,
        this_pointer: 0,
        arguments: [0; 16],
    };
    assert_eq!(
        engine.capture_active_resident_callback_call(KEY, active, 2, CallingConvention32::Cdecl, 0),
        Ok(expected_record)
    );
    let literal = words(&[
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        3,
        19,
        1,
        0,
        0x6200,
        0x8ff4,
        0x6008,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    let mut expected = before;
    expected[140..252].copy_from_slice(&literal);
    assert_eq!(engine.arena(), expected);
    assert_eq!(ram(&engine), before_ram);
    assert_eq!(
        PAGES.map(|pc| format!(
            "{:?}",
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(pc), 4096)
        )),
        before_versions
    );
    for (id, bytes, pointer) in modules {
        assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
        assert_eq!(
            engine.resident_bytes(id).unwrap().as_ptr() as usize,
            pointer
        );
    }
    let busy = HostError::Call(CallError::Busy);
    assert_eq!(engine.guard_resident(KEY, active), Err(busy));
    assert_eq!(engine.finish_resident_callback(KEY, home, 2), Err(busy));
    assert_eq!(
        engine.finish_resident_callback(KEY, active, 2),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), expected);
    engine.abort_callback(KEY, 2).unwrap();
    expected[..96].copy_from_slice(&frozen_outer);
    assert_eq!(engine.arena(), expected);
    assert_eq!(ram(&engine), before_ram);
}

use ring3_engine::{
    cpu::dbt::RegistryError,
    memory::{Access, FaultReason, MemoryError, MemoryFault},
    process::ResidentInstallation,
};

const ALL_PAGES: [u32; 9] = [
    0,
    0x3000,
    0x4000,
    0x5000,
    0x6000,
    0x7000,
    0x8000,
    0x9000,
    0xffff_f000,
];
const PCS: [u32; 7] = [0x4000, 0x5000, 0x5100, 0x5200, 0x6000, 0x6200, 0x7000];

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    home: u64,
    active: u64,
    foreign: u64,
    generation: u32,
}

fn call(error: CallError) -> HostError {
    HostError::Call(error)
}
fn invalid_unit() -> HostError {
    HostError::Resident(RegistryError::InvalidUnit)
}
fn stale() -> HostError {
    HostError::Resident(RegistryError::CodeInvalidated)
}

fn state_bytes(state: State32) -> Vec<u8> {
    let mut fields = vec![u32::from_le_bytes(*b"R3ST"), 0x10001, 56, 0];
    fields.extend(state.registers);
    fields.extend([state.eip, state.eflags]);
    words(&fields)
}

fn exit_bytes(reason: u32, retired: u32, detail: u32) -> Vec<u8> {
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

fn outer_state() -> State32 {
    State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: 0x4000,
        eflags: 0xcd7,
    }
}

fn stop(engine: &mut EngineInstance, state: State32, reason: u32, retired: u32, detail: u32) {
    // independent literal typed admission inputs; no wasm or guest execution is used.
    let arena = engine.arena_mut().unwrap();
    arena[..56].copy_from_slice(&state_bytes(state));
    arena[56..96].copy_from_slice(&exit_bytes(reason, retired, detail));
    arena[96..100].fill(0);
    arena[100..140].fill(0x5a);
    arena[140..].fill(0x6d);
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    let pairs: Vec<_> = blocks.iter().chain(gates).copied().collect();
    descriptors(engine, &pairs);
    engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get()
}

impl Fixture {
    fn new(home_ack: bool) -> Self {
        let mut engine = EngineInstance::new(ALL_PAGES.len() as u32, KEY).unwrap();
        for pc in ALL_PAGES {
            engine.map(pc, 1, 7).unwrap();
        }
        for (pc, bytes) in [
            (0x4000, &[0x0f, 0x0b][..]),
            (0x5000, &[0x90][..]),
            (0x5010, &[0x90][..]),
            (0x5100, &[0x0f, 0x0b][..]),
            (0x5200, &[0x0f, 0x0b][..]),
            (0x6000, &[0x90][..]),
            (0x6008, &[0x90][..]),
            (0x6200, &[0x0f, 0x0b][..]),
            (0x7000, &[0x90][..]),
        ] {
            engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
            engine.upload(pc, bytes.len() as u32).unwrap();
        }
        let outer = compile(&mut engine, &[(0x4000, 2)], &[(0x4000, 17)]);
        let home = compile(
            &mut engine,
            &[(0x5000, 1), (0x5010, 1), (0x5100, 2), (0x5200, 2)],
            &[(0x5100, 18), (0x5200, 20)],
        );
        let active = compile(
            &mut engine,
            &[(0x6000, 1), (0x6008, 1), (0x6200, 2)],
            &[(0x6200, 19)],
        );
        let foreign = compile(&mut engine, &[(0x7000, 1)], &[]);
        descriptors(
            &mut engine,
            &[
                (0x4000, 2),
                (0x5000, 1),
                (0x5100, 2),
                (0x5200, 2),
                (0x4000, 17),
                (0x5100, 18),
                (0x5200, 20),
            ],
        );
        let generation = engine.compile_with_gates(4, 3).unwrap();
        for (id, slot) in [(outer, 0), (active, 2), (foreign, 3)] {
            engine
                .acknowledge_resident_installation(KEY, id, slot)
                .unwrap();
        }
        if home_ack {
            engine
                .acknowledge_resident_installation(KEY, home, 1)
                .unwrap();
        }
        engine.write32(0x8ffc, 0x3005).unwrap();
        Self {
            engine,
            outer,
            home,
            active,
            foreign,
            generation,
        }
    }

    fn park(&mut self, replacement: bool, armed: bool, selected: bool) {
        stop(&mut self.engine, outer_state(), 8, 1, 17);
        let captured = if replacement {
            self.engine
                .capture_call(KEY, self.generation, CallingConvention32::Cdecl, 0)
        } else {
            self.engine
                .capture_resident_call(KEY, self.outer, CallingConvention32::Cdecl, 0)
        }
        .unwrap();
        assert_eq!(captured.token, 1);
        if replacement {
            assert_eq!(
                self.engine
                    .begin_callback(KEY, self.generation, 1, 0x5000, 0x5100, 18, &[])
                    .unwrap()
                    .token,
                2
            );
        } else {
            let receipt = self
                .engine
                .begin_resident_callback(KEY, self.outer, self.home, 1, 0x5000, 0x5100, 18, &[])
                .unwrap();
            assert_eq!(
                (receipt.token, receipt.outer_token, receipt.entry_esp),
                (2, 1, 0x8ff8)
            );
            if armed {
                self.engine
                    .authorize_resident_callback(KEY, self.home, 2)
                    .unwrap();
            }
            if selected {
                let mut state = outer_state();
                state.eip = 0x6000;
                state.registers[4] = 0x8ff8;
                stop(&mut self.engine, state, 3, 2, 0);
                self.engine
                    .select_resident_callback_unit(KEY, self.home, 2, self.active)
                    .unwrap();
            }
        }
    }

    fn inner(&mut self, home: bool, esp: u32, count: u32) -> State32 {
        // these typed stack controls may overlap public outer/callback words; private frames remain frozen.
        let return_pc = if home { 0x5010 } else { 0x6008 };
        self.engine.write32(esp, return_pc).unwrap();
        for index in 0..count {
            self.engine
                .write32(esp.wrapping_add(4 * (index + 1)), 0x1122_3300 + index)
                .unwrap();
        }
        let state = State32 {
            registers: [18, 0x1357_9bdf, 3, 4, esp, 6, 7, 8],
            eip: if home { 0x5200 } else { 0x6200 },
            eflags: 0xcd7,
        };
        stop(&mut self.engine, state, 8, 2, if home { 20 } else { 19 });
        state
    }
}

fn ready() -> Fixture {
    let mut f = Fixture::new(true);
    f.park(false, true, true);
    f.inner(false, 0x8ff4, 0);
    f
}

type Artifact = Result<(Vec<u8>, usize), HostError>;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Observed {
    arena: Vec<u8>,
    pointer: usize,
    generation: u32,
    modules: Vec<Artifact>,
    logical: Vec<Result<u64, HostError>>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    versions: Vec<Result<String, HostError>>,
}

fn observed(f: &Fixture) -> Observed {
    let e = &f.engine;
    let copy = |bytes: &[u8]| (bytes.to_vec(), bytes.as_ptr() as usize);
    let mut modules = vec![
        e.dispatcher_bytes(KEY).map(copy),
        e.artifact_bytes().map(copy),
    ];
    modules.extend([f.outer, f.home, f.active, f.foreign].map(|id| e.resident_bytes(id).map(copy)));
    Observed {
        arena: e.arena().to_vec(),
        pointer: e.arena_address(),
        generation: e.generation(),
        modules,
        logical: PCS
            .map(|pc| e.lookup_resident(pc).map(|id| id.get()))
            .to_vec(),
        installed: PCS.map(|pc| e.lookup_installed_resident(KEY, pc)).to_vec(),
        ram: ALL_PAGES
            .iter()
            .map(|&pc| {
                let mut bytes = vec![0; 4096];
                e.memory()?
                    .read(GuestAddress(pc), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .collect(),
        // snapshot Debug includes identity, first page and complete mapping/content versions.
        versions: ALL_PAGES
            .iter()
            .map(|&pc| {
                e.memory()?
                    .snapshot_code(GuestAddress(pc), 4096)
                    .map(|s| format!("{s:?}"))
                    .map_err(HostError::Memory)
            })
            .collect(),
    }
}

fn reject<T: std::fmt::Debug + PartialEq>(
    f: &mut Fixture,
    error: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<T, HostError>,
) {
    let before = observed(f);
    assert_eq!(operation(f), Err(error));
    assert_eq!(observed(f), before);
}

fn reject_admission(f: &mut Fixture, key: u64, unit: u64, token: u32, error: HostError) {
    reject(f, error, |f| {
        f.engine
            .capture_active_resident_callback_call_raw(key, unit, token, 0, 17)
    });
    reject(f, error, |f| {
        f.engine
            .complete_active_resident_callback_call(key, unit, token, 1, 0)
    });
}

fn capture_literal(
    f: &mut Fixture,
    home: bool,
    state: State32,
    tag: u32,
    count: u32,
    token: u32,
    old: bool,
) {
    let unit = if home { f.home } else { f.active };
    let record = CallRecord32 {
        token,
        id: if home { 20 } else { 19 },
        convention: tag,
        stack_words: count,
        gate_pc: state.eip,
        entry_esp: state.registers[4],
        return_pc: if home { 0x5010 } else { 0x6008 },
        this_pointer: if tag == 3 { 0x1357_9bdf } else { 0 },
        arguments: std::array::from_fn(|index| {
            if index < count as usize {
                0x1122_3300 + index as u32
            } else {
                0
            }
        }),
    };
    let mut values = vec![
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        token,
        record.id,
        tag,
        count,
        state.eip,
        state.registers[4],
        record.return_pc,
        record.this_pointer,
    ];
    values.extend(record.arguments);
    let mut expected = observed(f);
    let result = if old {
        f.engine
            .capture_resident_callback_call_raw(KEY, unit, 2, tag, count)
    } else {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, unit, 2, tag, count)
    };
    assert_eq!(result, Ok(record));
    expected.arena[140..252].copy_from_slice(&words(&values));
    assert_eq!(observed(f), expected);
    assert_eq!(
        f.engine.guard_resident(KEY, unit),
        Err(call(CallError::Busy))
    );
}

#[test]
fn active_capture_admission_and_gate_priorities_are_failure_atomic() {
    let mut f = ready();
    for (key, unit, token, error) in [
        (KEY ^ (1 << 32), f.active, 0, HostError::InvalidArtifact),
        (0, f.active, 2, HostError::InvalidArtifact),
        (KEY, f.active ^ (1 << 32), 2, invalid_unit()),
        (KEY, 0, 2, invalid_unit()),
        (KEY, f.active, 0, call(CallError::InvalidToken)),
        (KEY, f.active, 1, call(CallError::InvalidToken)),
        (KEY, f.active, 3, call(CallError::InvalidToken)),
        (KEY, f.home, 2, call(CallError::Busy)),
        (KEY, f.outer, 2, call(CallError::Busy)),
        (KEY, f.foreign, 2, call(CallError::Busy)),
    ] {
        reject_admission(&mut f, key, unit, token, error);
    }
    f.engine.arena_mut().unwrap()[0] = 0;
    for (tag, count) in [(0, 0), (4, 0), (1, 17), (0, 17)] {
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, call(CallError::InvalidRequest), |f| {
            f.engine
                .capture_active_resident_callback_call_raw(KEY, f.active, 2, tag, count)
        });
    }
    for (offset, value) in [
        (0, 0_u32),
        (4, 0x10009),
        (8, 55),
        (12, 1),
        (52, 0),
        (56, 0),
        (60, 0x10002),
        (64, 39),
        (68, 1),
        (72, 3),
        (80, 20),
        (84, 1),
        (88, 1),
        (92, 1),
    ] {
        let mut bad = ready();
        bad.engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        bad.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut bad, call(CallError::InvalidStop), |f| {
            f.engine
                .capture_active_resident_callback_call_raw(KEY, f.active, 2, 1, 0)
        });
    }
    let mut bad = ready();
    let mut state = outer_state();
    state.eip = 0x6000;
    state.registers[4] = 0x8ff4;
    stop(&mut bad.engine, state, 8, 2, 19);
    reject(&mut bad, call(CallError::InvalidStop), |f| {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, f.active, 2, 1, 0)
    });
    let mut home = Fixture::new(false);
    home.park(false, true, false);
    state.eip = 0x5100;
    state.registers[4] = 0x8ffc;
    stop(&mut home.engine, state, 8, 0, 18);
    reject(&mut home, call(CallError::InvalidStop), |f| {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, f.home, 2, 1, 0)
    });
    let mut cancelled = ready();
    state.eip = 0x6200;
    state.registers[4] = 0xa000;
    stop(&mut cancelled.engine, state, 8, 2, 19);
    cancelled.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(&mut cancelled, call(CallError::Cancelled), |f| {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, f.active, 2, 1, 0)
    });
    cancelled.engine.arena_mut().unwrap()[96..100].fill(0);
    let state = cancelled.inner(false, 0x8ff4, 0);
    capture_literal(&mut cancelled, false, state, 1, 0, 3, false);
    let mut fault = ready();
    fault.engine.unmap(0x9000, 1).unwrap();
    let mut state = outer_state();
    state.eip = 0x6200;
    state.registers[4] = 0x8ffc;
    stop(&mut fault.engine, state, 8, 2, 19);
    let error = call(CallError::Memory(MemoryError::Fault(MemoryFault {
        address: GuestAddress(0x9000),
        access: Access::Read,
        reason: FaultReason::Unmapped,
    })));
    reject(&mut fault, error, |f| {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, f.active, 2, 1, 1)
    });
    fault.engine.map(0x9000, 1, 7).unwrap();
    let state = fault.inner(false, 0x8ff4, 1);
    capture_literal(&mut fault, false, state, 1, 1, 3, false);
    for (replacement, armed) in [(true, false), (false, false)] {
        let mut blocked = Fixture::new(false);
        blocked.park(replacement, armed, false);
        let unit = blocked.home;
        reject_admission(
            &mut blocked,
            KEY,
            unit,
            2,
            call(if replacement {
                CallError::InvalidToken
            } else {
                CallError::Busy
            }),
        );
    }
    let mut absent = Fixture::new(true);
    let unit = absent.active;
    reject_admission(&mut absent, KEY, unit, 2, call(CallError::InvalidToken));
}

fn complete_literal(
    f: &mut Fixture,
    unit: u64,
    token: u32,
    result: u32,
    old: bool,
    output: State32,
) {
    let mut expected = observed(f);
    let completion = if old {
        f.engine
            .complete_resident_callback_call(KEY, unit, 2, token, result)
    } else {
        f.engine
            .complete_active_resident_callback_call(KEY, unit, 2, token, result)
    };
    assert_eq!(completion, Ok(()));
    expected.arena[..56].copy_from_slice(&state_bytes(output));
    expected.arena[56..96].copy_from_slice(&exit_bytes(3, 0, 0));
    assert_eq!(observed(f), expected);
    assert_eq!(f.engine.guard_resident(KEY, unit), Ok(()));
    assert_eq!(
        f.engine.guard_resident(KEY, f.outer),
        Err(call(CallError::Busy))
    );
    reject(f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_active_resident_callback_call(KEY, unit, 2, token, 0)
    });
}

#[test]
fn active_completion_uses_frozen_conventions_and_interoperates_at_home() {
    let rows = [
        (1, 0, 0x8ff4, 0x8ff8),
        (1, 1, 0x8ff4, 0x8ff8),
        (1, 16, 0x8f00, 0x8f04),
        (2, 0, 0x8ff4, 0x8ff8),
        (2, 1, 0x8ff4, 0x8ffc),
        (2, 16, 0x8f00, 0x8f44),
        (3, 1, 0x8ff4, 0x8ffc),
        (3, 16, 0x8f00, 0x8f44),
        (2, 1, 0xffff_fffc, 4),
    ];
    for (index, (tag, count, esp, completed_esp)) in rows.into_iter().enumerate() {
        let mut f = ready();
        let state = f.inner(false, esp, count);
        capture_literal(&mut f, false, state, tag, count, 3, false);
        if index == 0 {
            assert_eq!(
                f.engine.guard_dispatch_entry(KEY),
                Err(call(CallError::Busy))
            );
            assert_eq!(
                f.engine.guard(KEY, f.generation),
                Err(call(CallError::Busy))
            );
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine.select_resident_callback_unit(KEY, f.home, 2, 0)
            });
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine.finish_resident_callback(KEY, f.home, 2)
            });
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine
                    .capture_active_resident_callback_call_raw(KEY, f.active, 2, 0, 17)
            });
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine
                    .capture_resident_callback_call_raw(KEY, f.home, 2, 1, 0)
            });
            reject(&mut f, call(CallError::InvalidToken), |f| {
                f.engine
                    .complete_resident_callback_call(KEY, f.active, 2, 3, 0)
            });
            reject(&mut f, call(CallError::InvalidToken), |f| {
                f.engine.complete_resident_call(KEY, f.active, 3, 0)
            });
            reject(&mut f, call(CallError::InvalidToken), |f| {
                f.engine.complete_call(KEY, f.generation, 3, 0)
            });
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine.compile_resident(0)
            });
            reject(&mut f, call(CallError::Busy), |f| {
                f.engine.acknowledge_resident_installation(KEY, f.active, 2)
            });
            f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
            for (inner_token, error) in [
                (1, CallError::Busy),
                (0, CallError::InvalidToken),
                (2, CallError::InvalidToken),
                (4, CallError::InvalidToken),
            ] {
                reject(&mut f, call(error), |f| {
                    f.engine.complete_active_resident_callback_call(
                        KEY,
                        f.active,
                        2,
                        inner_token,
                        0,
                    )
                });
            }
            let frozen = f.engine.arena()[..96].to_vec();
            f.engine.arena_mut().unwrap()[16] ^= 1;
            reject(&mut f, call(CallError::StateChanged), |f| {
                f.engine
                    .complete_active_resident_callback_call(KEY, f.active, 2, 3, 0)
            });
            f.engine.arena_mut().unwrap()[..96].copy_from_slice(&frozen);
            f.engine.arena_mut().unwrap()[76] ^= 1;
            reject(&mut f, call(CallError::StateChanged), |f| {
                f.engine
                    .complete_active_resident_callback_call(KEY, f.active, 2, 3, 0)
            });
            f.engine.arena_mut().unwrap()[..96].copy_from_slice(&frozen);
            reject(&mut f, call(CallError::Cancelled), |f| {
                f.engine
                    .complete_active_resident_callback_call(KEY, f.active, 2, 3, 0)
            });
            f.engine.arena_mut().unwrap()[96..100].fill(0);
        }
        f.engine.arena_mut().unwrap()[140..300].fill(0x7c);
        if index == 0 {
            // no stack or argument memory remains readable; completion must use the frozen private frame.
            f.engine.unmap(0x8000, 1).unwrap();
        } else {
            f.engine.write32(esp, 0).unwrap();
            if count != 0 {
                f.engine.write32(esp.wrapping_add(4), 0).unwrap();
            }
        }
        let mut output = state;
        output.eip = 0x6008;
        output.registers[0] = 0xfedc_ba98;
        output.registers[4] = completed_esp;
        let unit = f.active;
        complete_literal(&mut f, unit, 3, 0xfedc_ba98, false, output);
        assert_eq!(
            f.engine.guard_resident(KEY, f.home),
            Err(call(CallError::Busy))
        );
    }
    // identical owners deliberately permit old capture/new completion and new capture/old completion at home.
    for (old_capture, old_complete) in [(true, false), (false, true)] {
        let mut f = Fixture::new(false);
        f.park(false, true, false);
        let state = f.inner(true, 0x8ff4, 1);
        capture_literal(&mut f, true, state, 3, 1, 3, old_capture);
        f.engine.write32(0x8ff4, 0).unwrap();
        f.engine.write32(0x8ff8, 0).unwrap();
        f.engine.arena_mut().unwrap()[140..300].fill(0x7c);
        let mut output = state;
        output.eip = 0x5010;
        output.registers[0] = 0;
        output.registers[4] = 0x8ffc;
        let unit = f.home;
        complete_literal(&mut f, unit, 3, 0, old_complete, output);
    }
    let mut cross = ready();
    cross.engine.write32(0x8ff4, 0x5100).unwrap();
    let mut expected = observed(&cross);
    let record = CallRecord32 {
        token: 3,
        id: 19,
        convention: 1,
        stack_words: 0,
        gate_pc: 0x6200,
        entry_esp: 0x8ff4,
        return_pc: 0x5100,
        this_pointer: 0,
        arguments: [0; 16],
    };
    assert_eq!(
        cross.engine.capture_active_resident_callback_call(
            KEY,
            cross.active,
            2,
            CallingConvention32::Cdecl,
            0
        ),
        Ok(record)
    );
    let mut literal = vec![
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        3,
        19,
        1,
        0,
        0x6200,
        0x8ff4,
        0x5100,
        0,
    ];
    literal.extend([0; 16]);
    expected.arena[140..252].copy_from_slice(&words(&literal));
    assert_eq!(observed(&cross), expected);
    let mut output = outer_state();
    output.eip = 0x5100;
    output.registers[0] = 44;
    output.registers[4] = 0x8ff8;
    let unit = cross.active;
    complete_literal(&mut cross, unit, 3, 44, false, output);
    assert_eq!(
        cross.engine.guard_resident(KEY, cross.home),
        Err(call(CallError::Busy))
    );
    let mut expected = observed(&cross);
    cross
        .engine
        .select_resident_callback_unit(KEY, cross.home, 2, cross.home)
        .unwrap();
    // selecting the immutable home changes only execution authority; installed lookup follows that authority.
    expected.installed = vec![
        Err(call(CallError::Busy)),
        Ok(ResidentInstallation {
            unit_id: cross.home,
            slot: 1,
        }),
        Ok(ResidentInstallation {
            unit_id: cross.home,
            slot: 1,
        }),
        Ok(ResidentInstallation {
            unit_id: cross.home,
            slot: 1,
        }),
        Err(call(CallError::Busy)),
        Err(call(CallError::Busy)),
        Err(call(CallError::Busy)),
    ];
    assert_eq!(observed(&cross), expected);
    assert_eq!(cross.engine.guard_resident(KEY, cross.home), Ok(()));
    assert_eq!(
        cross.engine.guard_resident(KEY, cross.active),
        Err(call(CallError::Busy))
    );
}

#[test]
fn active_inner_calls_preserve_parent_currency_and_neutral_recovery() {
    for (role_pc, pending) in [
        (0x4000, false),
        (0x5000, false),
        (0x6000, false),
        (0x4000, true),
        (0x5000, true),
        (0x6000, true),
    ] {
        let mut f = ready();
        if pending {
            let state = f.inner(false, 0x8ff4, 0);
            capture_literal(&mut f, false, state, 1, 0, 3, false);
        }
        f.engine.write32(role_pc, 0x9090_9090).unwrap();
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        let unit = f.active;
        reject_admission(&mut f, KEY, unit, 2, stale());
        reject_admission(&mut f, KEY ^ (1 << 32), unit, 0, HostError::InvalidArtifact);
        reject_admission(
            &mut f,
            KEY,
            unit,
            0,
            if role_pc == 0x6000 {
                stale()
            } else {
                call(CallError::InvalidToken)
            },
        );
        let mut expected = observed(&f);
        f.engine.abort_callback(KEY, 2).unwrap();
        expected.arena[..56].copy_from_slice(&state_bytes(outer_state()));
        expected.arena[56..96].copy_from_slice(&exit_bytes(8, 1, 17));
        // neutral revocation releases the active filter; current exact bindings become visible again.
        expected.installed = PCS
            .map(|pc| {
                let id = if pc == 0x4000 {
                    f.outer
                } else if pc < 0x6000 {
                    f.home
                } else if pc < 0x7000 {
                    f.active
                } else {
                    f.foreign
                };
                let slot = if id == f.outer {
                    0
                } else if id == f.home {
                    1
                } else if id == f.active {
                    2
                } else {
                    3
                };
                if pc / 4096 == role_pc / 4096 {
                    Err(stale())
                } else {
                    Ok(ResidentInstallation { unit_id: id, slot })
                }
            })
            .to_vec();
        assert_eq!(observed(&f), expected);
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.abandon_call(KEY, 3)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine
                .complete_active_resident_callback_call(KEY, f.foreign, 2, 3, 0)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.abort_callback(KEY, 2)
        });
    }
    for role_pc in [0x4000, 0x5000] {
        let mut f = Fixture::new(true);
        f.park(false, false, false);
        f.engine.write32(role_pc, 0x9090_9090).unwrap();
        let unit = f.foreign;
        reject_admission(&mut f, KEY, unit, 2, stale());
    }
    let mut stale_family = Fixture::new(true);
    stale_family.park(true, false, false);
    stale_family.engine.write32(0x5000, 0x9090_9090).unwrap();
    let unit = stale_family.home;
    reject_admission(&mut stale_family, KEY, unit, 2, stale());
    let mut f = ready();
    let state = f.inner(false, 0x8ff4, 0);
    capture_literal(&mut f, false, state, 1, 0, 3, false);
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.abandon_call(KEY, 1)
    });
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine.abandon_call(KEY, 0)
    });
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    let before = observed(&f);
    assert_eq!(f.engine.abandon_call(KEY, 3), Ok(()));
    assert_eq!(observed(&f), before);
    assert_eq!(f.engine.guard_resident(KEY, f.active), Ok(()));
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_active_resident_callback_call(KEY, f.active, 2, 3, 0)
    });
    reject(&mut f, call(CallError::Cancelled), |f| {
        f.engine
            .capture_active_resident_callback_call_raw(KEY, f.active, 2, 1, 0)
    });
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    capture_literal(&mut f, false, state, 1, 0, 4, false);
    let mut expected = observed(&f);
    f.engine.abort_callback(KEY, 2).unwrap();
    expected.arena[..56].copy_from_slice(&state_bytes(outer_state()));
    expected.arena[56..96].copy_from_slice(&exit_bytes(8, 1, 17));
    expected.installed = PCS
        .map(|pc| {
            let (unit_id, slot) = if pc == 0x4000 {
                (f.outer, 0)
            } else if pc < 0x6000 {
                (f.home, 1)
            } else if pc < 0x7000 {
                (f.active, 2)
            } else {
                (f.foreign, 3)
            };
            Ok(ResidentInstallation { unit_id, slot })
        })
        .to_vec();
    assert_eq!(observed(&f), expected);
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_active_resident_callback_call(KEY, f.active, 2, 4, 0)
    });
    assert_eq!(
        f.engine
            .begin_resident_callback(KEY, f.outer, f.home, 1, 0x5000, 0x5100, 18, &[])
            .unwrap()
            .token,
        5
    );
    let unit = f.home;
    reject_admission(&mut f, KEY, unit, 5, call(CallError::Busy));
    reject_admission(&mut f, KEY, unit, 2, call(CallError::InvalidToken));
    f.engine.abort_callback(KEY, 5).unwrap();
    let mut expected = observed(&f);
    assert_eq!(f.engine.complete_resident_call(KEY, f.outer, 1, 77), Ok(()));
    let mut output = outer_state();
    output.eip = 0x3005;
    output.registers[0] = 77;
    output.registers[4] = 0x9000;
    expected.arena[..56].copy_from_slice(&state_bytes(output));
    expected.arena[56..96].copy_from_slice(&exit_bytes(3, 0, 0));
    assert_eq!(observed(&f), expected);
    let mut closed = ready();
    let state = closed.inner(false, 0x8ff4, 0);
    capture_literal(&mut closed, false, state, 1, 0, 3, false);
    closed.engine.close();
    let unit = closed.active;
    reject_admission(&mut closed, 0, unit ^ (1 << 32), 0, HostError::Closed);
    reject(&mut closed, HostError::Closed, |f| {
        f.engine.abort_callback(0, 0)
    });
}
