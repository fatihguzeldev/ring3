use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};
use ring3_engine::{
    cpu::dbt::RegistryError,
    memory::{Access, FaultReason, MemoryError, MemoryFault},
    process::ResidentInstallation,
};

const KEY: u64 = 0x1020_3040_5060_7080;
const OUTER_ID: u32 = 0x8000_0011;
const RETURN_ID: u32 = 0x8000_0012;
const INNER_ID: u32 = 0x8000_0033;

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
fn callback_inner_call_captures_literal_frame_and_completes_only_frozen_inner_cpu() {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let pages = [0x3000, 0x4000, 0x5000, 0x8000, 0x9000];
    for pc in pages {
        engine.map(pc, 1, 7).unwrap();
    }
    for (pc, bytes) in [
        (0x3005, &[0x90][..]),
        (0x4000, &[0x0f, 0x0b][..]),
        (0x5000, &[0x90][..]),
        (0x500a, &[0x90][..]),
        (0x5100, &[0x0f, 0x0b][..]),
        (0x5200, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    engine.write32(0x8ffc, 0x3005).unwrap();
    descriptors(&mut engine, &[(0x4000, 2), (0x4000, OUTER_ID)]);
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    descriptors(
        &mut engine,
        &[
            (0x5000, 1),
            (0x500a, 1),
            (0x5100, 2),
            (0x5200, 2),
            (0x5100, RETURN_ID),
            (0x5200, INNER_ID),
        ],
    );
    let callback = engine.compile_resident_with_gates(4, 2).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, outer, 0)
        .unwrap();
    engine
        .acknowledge_resident_installation(KEY, callback, 1)
        .unwrap();
    let outer_state = State32 {
        registers: [0x89ab_cdef, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: 0x4000,
        eflags: 0xcd7,
    };
    // typed native Gate and stack controls; no guest instructions execute in this test.
    encode_state(&outer_state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: OUTER_ID },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[96..100].fill(0);
    let outer_call = engine
        .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(outer_call.token, 1);
    let admitted = engine
        .begin_resident_callback(
            KEY,
            outer,
            callback,
            1,
            0x5000,
            0x5100,
            RETURN_ID,
            &[0x1122_3344, 0x1234_5678],
        )
        .unwrap();
    assert_eq!(admitted.token, 2);
    assert_eq!(admitted.entry_esp, 0x8ff0);
    engine
        .authorize_resident_callback(KEY, callback, 2)
        .unwrap();
    engine.write32(0x8fe8, 0x500a).unwrap();
    engine.write32(0x8fec, 0x1122_3344).unwrap();
    let inner_state = State32 {
        registers: [0x1122_3344, 0x1357_9bdf, 3, 4, 0x8fe8, 6, 7, 8],
        eip: 0x5200,
        eflags: 0xcd7,
    };
    encode_state(&inner_state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: INNER_ID },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0x6d);
    let before = engine.arena().to_vec();
    let ram = pages.map(|pc| {
        let mut page = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(pc), &mut page)
            .unwrap();
        page
    });
    let modules = [outer, callback].map(|id| {
        let bytes = engine.resident_bytes(id).unwrap();
        (id, bytes.to_vec(), bytes.as_ptr() as usize)
    });
    let captured = engine
        .capture_resident_callback_call(KEY, callback, 2, CallingConvention32::Stdcall, 1)
        .unwrap();
    assert_eq!(captured.token, 3);
    assert_eq!(captured.id, INNER_ID);
    assert_eq!(captured.return_pc, 0x500a);
    assert_eq!(
        captured.arguments,
        [0x1122_3344, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let literal = words(&[
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        3,
        INNER_ID,
        2,
        1,
        0x5200,
        0x8fe8,
        0x500a,
        0,
        0x1122_3344,
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
    assert_eq!(
        engine.guard_resident(KEY, callback),
        Err(HostError::Call(CallError::Busy))
    );
    engine
        .complete_resident_callback_call(KEY, callback, 2, 3, 0xffff_fffd)
        .unwrap();
    let completed = words(&[
        u32::from_le_bytes(*b"R3ST"),
        0x10001,
        56,
        0,
        0xffff_fffd,
        0x1357_9bdf,
        3,
        4,
        0x8ff0,
        6,
        7,
        8,
        0x500a,
        0xcd7,
    ]);
    let exit = words(&[
        u32::from_le_bytes(*b"R3EX"),
        0x10003,
        40,
        0,
        3,
        0,
        0,
        0,
        0,
        0,
    ]);
    expected[..56].copy_from_slice(&completed);
    expected[56..96].copy_from_slice(&exit);
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.guard_resident(KEY, callback), Ok(()));
    assert_eq!(
        engine.guard_resident(KEY, outer),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        pages.map(|pc| {
            let mut page = vec![0; 4096];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(pc), &mut page)
                .unwrap();
            page
        }),
        ram
    );
    assert_eq!(
        [outer, callback].map(|id| {
            let bytes = engine.resident_bytes(id).unwrap();
            (id, bytes.to_vec(), bytes.as_ptr() as usize)
        }),
        modules
    );
    assert_eq!(
        engine.complete_resident_callback_call(KEY, callback, 2, 3, 0),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), expected);
}

const PAGES: [u32; 8] = [
    0,
    0x3000,
    0x4000,
    0x5000,
    0x6000,
    0x8000,
    0x9000,
    0xffff_f000,
];

struct Fixture {
    engine: EngineInstance,
    outer: u64,
    callback: u64,
    foreign: u64,
    generation: u32,
    outer_state: State32,
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

fn upload(engine: &mut EngineInstance, pc: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(pc, bytes.len() as u32).unwrap();
}

fn compile(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) -> u64 {
    let pairs: Vec<_> = blocks.iter().chain(gates).copied().collect();
    descriptors(engine, &pairs);
    engine
        .compile_resident_with_gates(blocks.len() as u32, gates.len() as u32)
        .unwrap()
        .get()
}

fn state_bytes(state: State32) -> Vec<u8> {
    let mut values = vec![u32::from_le_bytes(*b"R3ST"), 0x10001, 56, 0];
    values.extend(state.registers);
    values.extend([state.eip, state.eflags]);
    words(&values)
}

fn need_code() -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3EX"),
        0x10003,
        40,
        0,
        3,
        0,
        0,
        0,
        0,
        0,
    ])
}

fn typed_stop(engine: &mut EngineInstance, state: State32, id: u32, retired: u32) {
    // these are typed native admission controls, separate from actual wasm guest execution.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0x6d);
}

impl Fixture {
    fn new() -> Self {
        let mut engine = EngineInstance::new(PAGES.len() as u32, KEY).unwrap();
        for pc in PAGES {
            engine.map(pc, 1, 7).unwrap();
        }
        for (pc, bytes) in [
            (0x3005, &[0x90][..]),
            (0x4000, &[0x0f, 0x0b][..]),
            (0x4100, &[0x90][..]),
            (0x4200, &[0x0f, 0x0b][..]),
            (0x4300, &[0x0f, 0x0b][..]),
            (0x5000, &[0x90][..]),
            (0x500a, &[0x90][..]),
            (0x5100, &[0x0f, 0x0b][..]),
            (0x5200, &[0x0f, 0x0b][..]),
            (0x6000, &[0x0f, 0x0b][..]),
            (0x6100, &[0x90][..]),
            (0x6200, &[0x0f, 0x0b][..]),
            (0x6300, &[0x90][..]),
        ] {
            upload(&mut engine, pc, bytes);
        }
        let outer = compile(
            &mut engine,
            &[(0x4000, 2), (0x4100, 1), (0x4200, 2), (0x4300, 2)],
            &[(0x4000, OUTER_ID), (0x4200, RETURN_ID), (0x4300, INNER_ID)],
        );
        let callback = compile(
            &mut engine,
            &[(0x5000, 1), (0x500a, 1), (0x5100, 2), (0x5200, 2)],
            &[(0x5100, RETURN_ID), (0x5200, INNER_ID)],
        );
        let foreign = compile(&mut engine, &[(0x6300, 1)], &[]);
        descriptors(
            &mut engine,
            &[
                (0x6000, 2),
                (0x6100, 1),
                (0x6200, 2),
                (0x6000, 77),
                (0x6200, 78),
            ],
        );
        let generation = engine.compile_with_gates(3, 2).unwrap();
        assert_eq!(generation, 1);
        for (id, slot) in [(outer, 0), (callback, 1), (foreign, 2)] {
            engine
                .acknowledge_resident_installation(KEY, id, slot)
                .unwrap();
        }
        engine.write32(0x8ffc, 0x3005).unwrap();
        let outer_state = State32 {
            registers: [0x89ab_cdef, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
            eip: 0x4000,
            eflags: 0xcd7,
        };
        Self {
            engine,
            outer,
            callback,
            foreign,
            generation,
            outer_state,
        }
    }

    fn park(&mut self, armed: bool) {
        typed_stop(&mut self.engine, self.outer_state, OUTER_ID, 1);
        assert_eq!(
            self.engine
                .capture_resident_call(KEY, self.outer, CallingConvention32::Cdecl, 0)
                .unwrap()
                .token,
            1
        );
        let admitted = self
            .engine
            .begin_resident_callback(
                KEY,
                self.outer,
                self.callback,
                1,
                0x5000,
                0x5100,
                RETURN_ID,
                &[0x1122_3344, 0x1234_5678],
            )
            .unwrap();
        assert_eq!((admitted.token, admitted.entry_esp), (2, 0x8ff0));
        if armed {
            self.engine
                .authorize_resident_callback(KEY, self.callback, 2)
                .unwrap();
        }
    }

    fn inner(&mut self, esp: u32, retired: u32) -> State32 {
        // public stack controls may overlap callback/outer words; their private frames stay frozen.
        self.engine.write32(esp, 0x500a).unwrap();
        for index in 0..16_u32 {
            self.engine
                .write32(esp.wrapping_add(4 * (index + 1)), 0x1122_3300 + index)
                .unwrap();
        }
        let state = State32 {
            registers: [0x1122_3344, 0x1357_9bdf, 3, 4, esp, 6, 7, 8],
            eip: 0x5200,
            eflags: 0xcd7,
        };
        typed_stop(&mut self.engine, state, INNER_ID, retired);
        state
    }
}

type ObservedArtifact = Result<(Vec<u8>, usize), HostError>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observed {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    legacy: ObservedArtifact,
    dispatcher: ObservedArtifact,
    units: Vec<(u64, ObservedArtifact)>,
    installed: Vec<Result<ResidentInstallation, HostError>>,
    logical: Vec<Result<u64, HostError>>,
    snapshots: Vec<Result<String, HostError>>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    guards: Vec<Result<(), HostError>>,
}

fn observed(f: &Fixture) -> Observed {
    let e = &f.engine;
    Observed {
        arena: e.arena().to_vec(),
        arena_pointer: e.arena_address(),
        generation: e.generation(),
        legacy: e
            .artifact_bytes()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        dispatcher: e
            .dispatcher_bytes(KEY)
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        units: [f.outer, f.callback, f.foreign]
            .into_iter()
            .map(|id| {
                (
                    id,
                    e.resident_bytes(id)
                        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
                )
            })
            .collect(),
        installed: [0x4000, 0x5000, 0x5100, 0x5200, 0x6300]
            .into_iter()
            .map(|pc| e.lookup_installed_resident(KEY, pc))
            .collect(),
        logical: [0x4000, 0x5000, 0x5100, 0x5200, 0x6300]
            .into_iter()
            .map(|pc| e.lookup_resident(pc).map(|id| id.get()))
            .collect(),
        snapshots: [0x3000, 0x4000, 0x5000, 0x6000]
            .into_iter()
            .map(|pc| {
                e.memory().and_then(|memory| {
                    memory
                        .snapshot_code(GuestAddress(pc), 4096)
                        .map(|snapshot| format!("{snapshot:?}"))
                        .map_err(HostError::Memory)
                })
            })
            .collect(),
        ram: PAGES
            .into_iter()
            .map(|pc| {
                let mut page = vec![0; 4096];
                e.memory()
                    .and_then(|memory| {
                        memory
                            .read(GuestAddress(pc), &mut page)
                            .map_err(HostError::Memory)
                    })
                    .map(|()| page)
            })
            .collect(),
        guards: vec![
            e.guard_resident(KEY, f.outer),
            e.guard_resident(KEY, f.callback),
            e.guard_resident(KEY, f.foreign),
            e.guard(KEY, f.generation),
            e.guard_dispatch_entry(KEY),
        ],
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

fn capture(f: &mut Fixture, state: State32, tag: u32, count: u32, token: u32) {
    let mut expected = observed(f);
    let mut literal = vec![
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        token,
        INNER_ID,
        tag,
        count,
        0x5200,
        state.registers[4],
        0x500a,
        if tag == 3 { 0x1357_9bdf } else { 0 },
    ];
    literal.extend((0..16).map(|i| if i < count { 0x1122_3300 + i } else { 0 }));
    let record = f
        .engine
        .capture_resident_callback_call_raw(KEY, f.callback, 2, tag, count)
        .unwrap();
    assert_eq!(
        (
            record.token,
            record.id,
            record.convention,
            record.stack_words,
            record.gate_pc,
            record.entry_esp,
            record.return_pc,
            record.this_pointer
        ),
        (
            token,
            INNER_ID,
            tag,
            count,
            0x5200,
            state.registers[4],
            0x500a,
            if tag == 3 { 0x1357_9bdf } else { 0 }
        )
    );
    assert_eq!(
        record.arguments,
        std::array::from_fn(|i| if i < count as usize {
            0x1122_3300 + i as u32
        } else {
            0
        })
    );
    expected.arena[140..252].copy_from_slice(&words(&literal));
    expected.guards[1] = Err(call(CallError::Busy));
    expected.guards[4] = Err(call(CallError::Busy));
    assert_eq!(observed(f), expected);
}

fn complete(
    f: &mut Fixture,
    state: State32,
    tag: u32,
    count: u32,
    token: u32,
    result: u32,
) -> State32 {
    let mut expected = observed(f);
    let mut output = state;
    output.registers[0] = result;
    output.registers[4] = state.registers[4].wrapping_add(4 + if tag == 1 { 0 } else { 4 * count });
    output.eip = 0x500a;
    f.engine
        .complete_resident_callback_call(KEY, f.callback, 2, token, result)
        .unwrap();
    expected.arena[..56].copy_from_slice(&state_bytes(output));
    expected.arena[56..96].copy_from_slice(&need_code());
    expected.guards[1] = Ok(());
    expected.guards[4] = Ok(());
    assert_eq!(observed(f), expected);
    output
}

#[test]
fn callback_inner_admission_priorities_preserve_every_public_observation() {
    let mut f = Fixture::new();
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 0, 99)
    });
    f.park(false);
    for inner in [0, 1, 3] {
        reject(&mut f, call(CallError::Busy), |f| {
            f.engine
                .complete_resident_callback_call(KEY, f.callback, 2, inner, 0)
        });
    }
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 0, 99)
    });
    f.engine
        .authorize_resident_callback(KEY, f.callback, 2)
        .unwrap();
    let state = f.inner(0x8fe8, 3);
    for (key, id, token, error) in [
        (
            KEY ^ (1_u64 << 32),
            f.callback,
            2,
            HostError::InvalidArtifact,
        ),
        (KEY, 0, 2, invalid_unit()),
        (KEY, f.callback ^ (1_u64 << 32), 2, invalid_unit()),
        (KEY, f.outer, 2, call(CallError::InvalidToken)),
        (KEY, f.foreign, 2, call(CallError::InvalidToken)),
        (KEY, f.callback, 0, call(CallError::InvalidToken)),
        (KEY, f.callback, 1, call(CallError::InvalidToken)),
        (KEY, f.callback, 3, call(CallError::InvalidToken)),
    ] {
        reject(&mut f, error, |f| {
            f.engine
                .capture_resident_callback_call_raw(key, id, token, 0, 99)
        });
        reject(&mut f, error, |f| {
            f.engine
                .complete_resident_callback_call(key, id, token, 1, 0)
        });
    }
    for (tag, count) in [(0, 0), (4, 0), (1, 17), (u32::MAX, u32::MAX)] {
        reject(&mut f, call(CallError::InvalidRequest), |f| {
            f.engine
                .capture_resident_callback_call_raw(KEY, f.callback, 2, tag, count)
        });
    }
    let canonical = f.engine.arena()[..96].to_vec();
    for offset in [0, 52, 56, 80] {
        f.engine.arena_mut().unwrap()[offset] ^= 0x80;
        if offset == 52 {
            f.engine.arena_mut().unwrap()[52..56].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
        }
        reject(&mut f, call(CallError::InvalidStop), |f| {
            f.engine
                .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 0)
        });
        f.engine.arena_mut().unwrap()[..96].copy_from_slice(&canonical);
    }
    for (pc, id) in [
        (0x5000, INNER_ID),
        (0x5100, RETURN_ID),
        (0x5100, INNER_ID),
        (0x4000, OUTER_ID),
        (0x6300, INNER_ID),
    ] {
        typed_stop(&mut f.engine, State32 { eip: pc, ..state }, id, 0);
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, call(CallError::InvalidStop), |f| {
            f.engine
                .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 0)
        });
    }
    typed_stop(&mut f.engine, state, INNER_ID, 3);
    f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    reject(&mut f, call(CallError::InvalidRequest), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 0, 17)
    });
    reject(&mut f, call(CallError::Cancelled), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 0)
    });
    f.engine.arena_mut().unwrap()[96..100].fill(0);
    typed_stop(
        &mut f.engine,
        State32 {
            registers: [0x1122_3344, 0x1357_9bdf, 3, 4, 0xa000, 6, 7, 8],
            ..state
        },
        INNER_ID,
        0,
    );
    reject(&mut f, call(CallError::InvalidRequest), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 17)
    });
    reject(
        &mut f,
        call(CallError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0xa000),
            access: Access::Read,
            reason: FaultReason::Unmapped,
        }))),
        |f| {
            f.engine
                .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 0)
        },
    );
    let boundary = f.inner(0x8ffc, 3);
    let ram_before_permission = observed(&f).ram;
    f.engine.protect(0x9000, 1, 2).unwrap();
    reject(
        &mut f,
        call(CallError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(0x9000),
            access: Access::Read,
            reason: FaultReason::Permission,
        }))),
        |f| {
            f.engine
                .capture_resident_callback_call_raw(KEY, f.callback, 2, 2, 1)
        },
    );
    f.engine.protect(0x9000, 1, 7).unwrap();
    assert_eq!(observed(&f).ram, ram_before_permission);
    capture(&mut f, boundary, 2, 1, 3);

    let mut replacement = Fixture::new();
    typed_stop(
        &mut replacement.engine,
        State32 {
            eip: 0x6000,
            ..replacement.outer_state
        },
        77,
        0,
    );
    replacement
        .engine
        .capture_call(KEY, replacement.generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    replacement
        .engine
        .begin_callback(KEY, replacement.generation, 1, 0x6100, 0x6200, 78, &[])
        .unwrap();
    reject(&mut replacement, call(CallError::InvalidToken), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 0, 99)
    });
    reject(&mut replacement, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 1, 0)
    });

    let mut same = Fixture::new();
    typed_stop(&mut same.engine, same.outer_state, OUTER_ID, 0);
    same.engine
        .capture_resident_call(KEY, same.outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    same.engine
        .begin_resident_callback(
            KEY,
            same.outer,
            same.outer,
            1,
            0x4100,
            0x4200,
            RETURN_ID,
            &[],
        )
        .unwrap();
    reject(&mut same, call(CallError::InvalidRequest), |f| {
        f.engine.authorize_resident_callback(KEY, f.outer, 2)
    });
    reject(&mut same, call(CallError::Busy), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.outer, 2, 0, 99)
    });
    reject(&mut same, call(CallError::Busy), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.outer, 2, 1, 0)
    });

    for invalidate_callback in [false, true] {
        let mut f = Fixture::new();
        f.park(true);
        f.inner(0x8fe8, 3);
        let (pc, value) = if invalidate_callback {
            (0x5000, 0x90)
        } else {
            (0x4000, 0x0b0f)
        };
        f.engine.write32(pc, value).unwrap();
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        for token in [0, 2] {
            let error = if invalidate_callback || token == 2 {
                stale()
            } else {
                call(CallError::InvalidToken)
            };
            reject(&mut f, error, |f| {
                f.engine
                    .capture_resident_callback_call_raw(KEY, f.callback, token, 0, 99)
            });
            reject(&mut f, error, |f| {
                f.engine
                    .complete_resident_callback_call(KEY, f.callback, token, 0, 0)
            });
        }
    }
}

#[test]
fn callback_inner_conventions_use_frozen_frames_and_never_reread_transfer_or_stack() {
    for (tag, count, esp, completed_esp, result) in [
        (1, 0, 0x8fe8, 0x8fec, 0),
        (1, 1, 0x8fe8, 0x8fec, u32::MAX),
        (1, 16, 0xffff_fffc, 0, 0x8000_0000),
        (2, 0, 0x8fe8, 0x8fec, 0),
        (2, 1, 0x8fe8, 0x8ff0, u32::MAX),
        (2, 16, 0xffff_fffc, 0x40, 0x8000_0000),
        (3, 0, 0x8fe8, 0x8fec, 0),
        (3, 1, 0x8fe8, 0x8ff0, u32::MAX),
        (3, 16, 0xffff_fffc, 0x40, 0x8000_0000),
    ] {
        let mut f = Fixture::new();
        f.park(true);
        let state = f.inner(esp, if count == 16 { u32::MAX } else { 0 });
        capture(&mut f, state, tag, count, 3);
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.complete_resident_call(KEY, f.callback, 3, result)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.complete_resident_call(KEY, f.outer, 3, result)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.complete_call(KEY, f.generation, 3, result)
        });
        if tag == 1 && count == 0 {
            let stopped = f.engine.arena()[..96].to_vec();
            for offset in (0..96).step_by(4) {
                f.engine.arena_mut().unwrap()[offset] ^= 1;
                f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
                for wrong in [0, 2, 4, u32::MAX] {
                    reject(&mut f, call(CallError::InvalidToken), |f| {
                        f.engine
                            .complete_resident_callback_call(KEY, f.callback, 2, wrong, result)
                    });
                }
                reject(&mut f, call(CallError::Busy), |f| {
                    f.engine
                        .complete_resident_callback_call(KEY, f.callback, 2, 1, result)
                });
                reject(&mut f, call(CallError::StateChanged), |f| {
                    f.engine
                        .complete_resident_callback_call(KEY, f.callback, 2, 3, result)
                });
                f.engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
            }
            reject(&mut f, call(CallError::Cancelled), |f| {
                f.engine
                    .complete_resident_callback_call(KEY, f.callback, 2, 3, result)
            });
            f.engine.arena_mut().unwrap()[96..100].fill(0);
        }
        // deliberately replace public frame bytes after capture; private frozen completion ignores them.
        f.engine.write32(esp, 0xdead_beef).unwrap();
        f.engine.write32(esp.wrapping_add(4), 0xcafe_babe).unwrap();
        f.engine.arena_mut().unwrap()[100..140].fill(0xc3);
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0xa5);
        let ram = observed(&f).ram;
        let protected = if esp == 0xffff_fffc {
            [0xffff_f000, 0]
        } else {
            [0x8000, 0x9000]
        };
        for pc in protected {
            f.engine.protect(pc, 1, 2).unwrap();
        }
        let output = complete(&mut f, state, tag, count, 3, result);
        assert_eq!(output.registers[4], completed_esp);
        assert_eq!(output.registers[0], result);
        for pc in protected {
            f.engine.protect(pc, 1, 7).unwrap();
        }
        assert_eq!(observed(&f).ram, ram);
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine
                .complete_resident_callback_call(KEY, f.callback, 2, 3, result)
        });
        reject(&mut f, call(CallError::Busy), |f| {
            f.engine
                .complete_resident_callback_call(KEY, f.callback, 2, 1, result)
        });
    }

    // return words are frozen u32 values; this slice adds no continuation-membership rule.
    let mut f = Fixture::new();
    f.park(true);
    let state = f.inner(0x8fe8, 0);
    f.engine.write32(0x8fe8, 0xdead_beef).unwrap();
    let mut expected = observed(&f);
    let captured = f
        .engine
        .capture_resident_callback_call_raw(KEY, f.callback, 2, 1, 0)
        .unwrap();
    assert_eq!((captured.token, captured.return_pc), (3, 0xdead_beef));
    expected.arena[140..252].copy_from_slice(&words(&[
        u32::from_le_bytes(*b"R3CF"),
        0x10001,
        112,
        0,
        3,
        INNER_ID,
        1,
        0,
        0x5200,
        0x8fe8,
        0xdead_beef,
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
    ]));
    expected.guards[1] = Err(call(CallError::Busy));
    expected.guards[4] = Err(call(CallError::Busy));
    assert_eq!(observed(&f), expected);
    f.engine
        .complete_resident_callback_call(KEY, f.callback, 2, 3, 7)
        .unwrap();
    let output = State32 {
        registers: [7, 0x1357_9bdf, 3, 4, 0x8fec, 6, 7, 8],
        eip: 0xdead_beef,
        ..state
    };
    expected.arena[..56].copy_from_slice(&state_bytes(output));
    expected.arena[56..96].copy_from_slice(&need_code());
    expected.guards[1] = Ok(());
    expected.guards[4] = Ok(());
    assert_eq!(observed(&f), expected);
}

fn outer_exit() -> Vec<u8> {
    words(&[
        u32::from_le_bytes(*b"R3EX"),
        0x10003,
        40,
        0,
        8,
        1,
        OUTER_ID,
        0,
        0,
        0,
    ])
}

fn restore_outer_observation(
    f: &Fixture,
    mut expected: Observed,
    current: [Result<(), HostError>; 3],
) -> Observed {
    expected.arena[..56].copy_from_slice(&state_bytes(f.outer_state));
    expected.arena[56..96].copy_from_slice(&outer_exit());
    expected.installed = [0x4000, 0x5000, 0x5100, 0x5200, 0x6300]
        .into_iter()
        .map(|pc| {
            let (unit_id, slot) = if pc == 0x4000 {
                (f.outer, 0)
            } else if pc == 0x6300 {
                (f.foreign, 2)
            } else {
                (f.callback, 1)
            };
            current[slot as usize].map(|()| ResidentInstallation { unit_id, slot })
        })
        .collect();
    expected.guards = current
        .into_iter()
        .map(|currency| currency.and(Err(call(CallError::Busy))))
        .chain([Err(call(CallError::Busy)), Err(call(CallError::Busy))])
        .collect();
    expected
}

#[test]
fn callback_inner_pending_parks_execution_and_recovers_without_replaying_authority() {
    let mut f = Fixture::new();
    f.park(true);
    let state = f.inner(0x8fe8, 3);
    capture(&mut f, state, 2, 1, 3);
    for id in [f.outer, f.callback, f.foreign] {
        reject(&mut f, call(CallError::Busy), |f| {
            f.engine.guard_resident(KEY, id)
        });
        reject(&mut f, call(CallError::Busy), |f| {
            f.engine.store_resident32(KEY, id, 0xa000, 7)
        });
    }
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.guard_dispatch_entry(KEY)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.guard(KEY, f.generation)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.finish_resident_callback(KEY, f.callback, 2)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY, f.callback, 2, 0, 99)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.capture_resident_call_raw(KEY, f.callback, 0, 99)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.capture_call_raw(KEY, f.generation, 0, 99)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 1, 7)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.complete_resident_call(KEY, f.outer, 1, 7)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.abandon_call(KEY, 1)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.begin_resident_callback(
            KEY,
            f.outer,
            f.callback,
            1,
            0x5000,
            0x5100,
            RETURN_ID,
            &[],
        )
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.compile_resident_with_gates(0, 0)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine.compile_with_gates(0, 0)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine
            .acknowledge_resident_installation(KEY, f.callback, u32::MAX)
    });
    let before_inspection = observed(&f);
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, 0x5200),
        Ok(ResidentInstallation {
            unit_id: f.callback,
            slot: 1
        })
    );
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, 0x4000),
        Err(call(CallError::Busy))
    );
    assert_eq!(
        f.engine.lookup_installed_resident(KEY, 0x6300),
        Err(call(CallError::Busy))
    );
    assert_eq!(observed(&f), before_inspection);

    let mut abandoned = observed(&f);
    f.engine.abandon_call(KEY, 3).unwrap();
    abandoned.guards[1] = Ok(());
    abandoned.guards[4] = Ok(());
    assert_eq!(observed(&f), abandoned);
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 3, 7)
    });
    reject(&mut f, call(CallError::Busy), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 1, 7)
    });
    capture(&mut f, state, 2, 1, 4);
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 3, 7)
    });
    complete(&mut f, state, 2, 1, 4, 0xffff_fffd);
    let returned = State32 {
        registers: [0x8000_0042, 0xffff_ffff, 3, 4, 0x8ffc, 6, 7, 8],
        eip: 0x5100,
        eflags: 2,
    };
    typed_stop(&mut f.engine, returned, RETURN_ID, 0);
    let mut finished = restore_outer_observation(&f, observed(&f), [Ok(()); 3]);
    let receipt = f
        .engine
        .finish_resident_callback(KEY, f.callback, 2)
        .unwrap();
    assert_eq!(
        (
            receipt.token,
            receipt.outer_token,
            receipt.result,
            receipt.outer_unit_id,
            receipt.callback_unit_id
        ),
        (2, 1, 0x8000_0042, f.outer, f.callback)
    );
    finished.arena[140..188].copy_from_slice(&words(&[
        u32::from_le_bytes(*b"R3RR"),
        0x10001,
        48,
        0,
        2,
        1,
        0x8000_0042,
        0,
        f.outer as u32,
        (f.outer >> 32) as u32,
        f.callback as u32,
        (f.callback >> 32) as u32,
    ]));
    assert_eq!(observed(&f), finished);
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.callback, 2, 4, 7)
    });
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine.abort_callback(KEY, 2)
    });
    let mut completed = observed(&f);
    f.engine
        .complete_resident_call(KEY, f.outer, 1, receipt.result)
        .unwrap();
    let outer_completed = State32 {
        registers: [0x8000_0042, 0x1357_9bdf, 3, 4, 0x9000, 6, 7, 8],
        eip: 0x3005,
        eflags: 0xcd7,
    };
    completed.arena[..56].copy_from_slice(&state_bytes(outer_completed));
    completed.arena[56..96].copy_from_slice(&need_code());
    completed.guards.fill(Ok(()));
    assert_eq!(observed(&f), completed);
    typed_stop(&mut f.engine, f.outer_state, OUTER_ID, 1);
    assert_eq!(
        f.engine
            .capture_resident_call(KEY, f.outer, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        5
    );
    f.engine.abandon_call(KEY, 5).unwrap();

    for invalidated in [0, 1, 2] {
        let mut f = Fixture::new();
        f.park(true);
        let state = f.inner(0x8fe8, 3);
        capture(&mut f, state, 2, 1, 3);
        f.engine.arena_mut().unwrap()[0] ^= 0x80;
        f.engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        if invalidated == 1 {
            f.engine.write32(0x4000, 0x0b0f).unwrap();
        }
        if invalidated == 2 {
            f.engine.write32(0x5000, 0x90).unwrap();
        }
        reject(
            &mut f,
            if invalidated == 0 {
                call(CallError::StateChanged)
            } else {
                stale()
            },
            |f| {
                f.engine
                    .complete_resident_callback_call(KEY, f.callback, 2, 3, 7)
            },
        );
        let current = [
            if invalidated == 1 {
                Err(stale())
            } else {
                Ok(())
            },
            if invalidated == 2 {
                Err(stale())
            } else {
                Ok(())
            },
            Ok(()),
        ];
        let aborted = restore_outer_observation(&f, observed(&f), current);
        f.engine.abort_callback(KEY, 2).unwrap();
        assert_eq!(observed(&f), aborted);
        let dead = if invalidated == 2 {
            stale()
        } else {
            call(CallError::InvalidToken)
        };
        reject(&mut f, dead, |f| {
            f.engine
                .complete_resident_callback_call(KEY, f.callback, 2, 3, 7)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.abort_callback(KEY, 2)
        });
        reject(&mut f, call(CallError::InvalidToken), |f| {
            f.engine.abandon_call(KEY, 3)
        });
        reject(
            &mut f,
            if invalidated == 1 {
                stale()
            } else {
                call(CallError::Cancelled)
            },
            |f| f.engine.complete_resident_call(KEY, f.outer, 1, 7),
        );
        let mut abandoned = observed(&f);
        f.engine.abandon_call(KEY, 1).unwrap();
        abandoned.guards = current.into_iter().chain([Ok(()), Ok(())]).collect();
        assert_eq!(observed(&f), abandoned);
    }

    let mut f = Fixture::new();
    f.park(true);
    let state = f.inner(0x8fe8, 3);
    capture(&mut f, state, 2, 1, 3);
    let arena = f.engine.arena().to_vec();
    let pointer = f.engine.arena_address();
    f.engine.close();
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(f.engine.arena_address(), pointer);
    reject(&mut f, HostError::Closed, |f| {
        f.engine
            .capture_resident_callback_call_raw(KEY ^ 1, 0, 0, 0, u32::MAX)
    });
    reject(&mut f, HostError::Closed, |f| {
        f.engine
            .complete_resident_callback_call(KEY ^ 1, 0, 0, 1, 7)
    });
    reject(&mut f, HostError::Closed, |f| {
        f.engine.abort_callback(KEY ^ 1, 2)
    });
    reject(&mut f, HostError::Closed, |f| {
        f.engine.abandon_call(KEY ^ 1, 3)
    });
}
