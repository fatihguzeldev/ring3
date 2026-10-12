use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, X87_OFFSET},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{Access, BackingOffset, GuestAddress, MemoryError},
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0xa539_1234_5678_abcd;
const CODE: u32 = 0x1000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x9000;
const CURRENT: u32 = 0x0001_0009;

#[derive(Clone, Copy)]
enum Owner {
    Replacement,
    Resident,
}

#[test]
fn current_process_has_the_named_numeric_stdcall_zero_word_contract() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        let api = WindowsApi32::resolve(module, "GetCurrentProcess")
            .expect("the named current-process provider is admitted");
        assert_eq!(api.id(), CURRENT);
        assert_eq!(WindowsApi32::from_id(CURRENT), Some(api));
        assert_eq!(api.convention(), CallingConvention32::Stdcall);
        assert_eq!(api.stack_words(), 0);
    }
}

#[test]
fn current_process_raw_id_completes_both_saved_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        engine.upload(CODE, 2).unwrap();
        for (index, value) in [CODE, 2, CODE, CURRENT].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let unit = match owner {
            Owner::Replacement => u64::from(engine.compile_with_gates(1, 1).unwrap()),
            Owner::Resident => engine.compile_resident_with_gates(1, 1).unwrap().get(),
        };
        engine.write32(ESP, RETURN).unwrap();
        let state = State32 {
            registers: [0x89ab_cdef, 1, 2, 3, ESP, 5, 6, 7],
            eip: CODE,
            eflags: 0xcd7,
        };
        let arena = engine.arena_mut().unwrap();
        encode_state(&state, &mut arena[..56]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 7,
                reason: ExitReason::Gate { id: CURRENT },
            },
            &mut arena[56..96],
        )
        .unwrap();
        arena[96..100].fill(0);
        let call = match owner {
            Owner::Replacement => {
                engine.capture_call(KEY, unit as u32, CallingConvention32::Stdcall, 0)
            }
            Owner::Resident => {
                engine.capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 0)
            }
        }
        .unwrap();
        assert_eq!(call.id, CURRENT);
        assert_eq!(call.stack_words, 0);
        let mut expected = engine.arena().to_vec();
        let mut next = state;
        next.registers[0] = u32::MAX;
        next.registers[4] = ESP + 4;
        next.eip = RETURN;
        encode_state(&next, &mut expected[..56]).unwrap();
        encode_exit_v3(
            &ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode,
            },
            &mut expected[56..96],
        )
        .unwrap();
        let completed = match owner {
            Owner::Replacement => engine.complete_windows_call(KEY, unit as u32, call.token),
            Owner::Resident => engine.complete_resident_windows_call(KEY, unit, call.token),
        };
        assert_eq!(completed, Ok(()));
        assert_eq!(engine.arena(), expected);
    }
}

const KEEP: u32 = 0x3000;
const STALE: u32 = 0x5000;
const DATA: u32 = 0xa000;
const TOP_PAGE: u32 = 0xffff_f000;
const GET: u32 = 0x10001;
const SET: u32 = 0x10002;
const LAST_ERROR: u32 = 0x89ab_cdef;
const APIS: [u32; 3] = [CURRENT, GET, SET];
const PAGES: [u32; 6] = [CODE, KEEP, STALE, 0x8000, DATA, TOP_PAGE];

struct Fixture {
    engine: EngineInstance,
    generation: u32,
    caller: u64,
    keeper: u64,
    stale: u64,
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance) {
    let fields = APIS
        .into_iter()
        .enumerate()
        .flat_map(|(i, _)| [CODE + i as u32 * 16, 2])
        .chain(
            APIS.into_iter()
                .enumerate()
                .flat_map(|(i, id)| [CODE + i as u32 * 16, id]),
        );
    for (i, value) in fields.enumerate() {
        let at = TRANSFER_OFFSET + i * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture(owner: Owner) -> Fixture {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for address in PAGES {
        engine
            .map(
                address,
                1,
                if [CODE, KEEP, STALE].contains(&address) {
                    7
                } else {
                    3
                },
            )
            .unwrap();
    }
    for i in 0..APIS.len() {
        upload(&mut engine, CODE + i as u32 * 16, &[0x0f, 0x0b]);
    }
    upload(&mut engine, KEEP, &[0x90]);
    upload(&mut engine, STALE, &[0x90]);
    upload(&mut engine, DATA, &[0x55, 0xa5, 0x5a, 0xaa]);
    describe(&mut engine);
    let caller = engine.compile_resident_with_gates(3, 3).unwrap().get();
    let mut others = Vec::new();
    for address in [KEEP, STALE] {
        for (i, value) in [address, 1].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + i * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        others.push(engine.compile_resident(1).unwrap().get());
    }
    describe(&mut engine);
    let generation = engine.compile_with_gates(3, 3).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (i, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (i as u8).wrapping_mul(29).wrapping_add(17);
    }
    let mut f = Fixture {
        engine,
        generation,
        caller,
        keeper: others[0],
        stale: others[1],
    };
    let (token, state) = capture(
        &mut f,
        owner,
        SET,
        CallingConvention32::Stdcall,
        1,
        ESP,
        0xcd7,
    );
    returned(
        &mut f,
        owner,
        token,
        state,
        state.registers[0],
        CallingConvention32::Stdcall,
        1,
    );
    f
}

fn capture(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    convention: CallingConvention32,
    count: u32,
    esp: u32,
    flags: u32,
) -> (u32, State32) {
    f.engine.write32(esp, RETURN).unwrap();
    for i in 0..count {
        f.engine.write32(esp + 4 + i * 4, LAST_ERROR).unwrap();
    }
    let state = State32 {
        registers: [
            0x7654_3210,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: CODE + APIS.iter().position(|&value| value == id).unwrap() as u32 * 16,
        eflags: flags,
    };
    let arena = f.engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    let call = match owner {
        Owner::Replacement => f.engine.capture_call(KEY, f.generation, convention, count),
        Owner::Resident => f
            .engine
            .capture_resident_call(KEY, f.caller, convention, count),
    }
    .unwrap();
    assert_eq!(call.stack_words, count);
    (call.token, state)
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.caller, token),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    mapped_pages: u32,
    generation: u32,
    ram: Vec<Result<Vec<u8>, MemoryError>>,
    access: Vec<Result<BackingOffset, MemoryError>>,
    modules: Vec<(Vec<u8>, usize)>,
}

fn saved(f: &Fixture) -> Saved {
    let memory = f.engine.memory().unwrap();
    Saved {
        arena: f.engine.arena().to_vec(),
        arena_pointer: f.engine.arena_address(),
        mapped_pages: memory.mapped_pages(),
        generation: f.engine.generation(),
        ram: PAGES
            .into_iter()
            .map(|address| {
                let mut bytes = vec![0; 4096];
                memory
                    .read(GuestAddress(address), &mut bytes)
                    .map(|_| bytes)
            })
            .collect(),
        access: PAGES
            .into_iter()
            .flat_map(|address| {
                [Access::Read, Access::Write, Access::Execute]
                    .map(|access| memory.resolve(GuestAddress(address), access))
            })
            .collect(),
        modules: [
            f.engine.artifact_bytes().unwrap(),
            f.engine.resident_bytes(f.caller).unwrap(),
            f.engine.resident_bytes(f.keeper).unwrap(),
        ]
        .into_iter()
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
        .collect(),
    }
}

fn expected_return(
    f: &Fixture,
    state: State32,
    value: u32,
    convention: CallingConvention32,
    count: u32,
) -> Saved {
    let mut expected = saved(f);
    let mut next = state;
    next.registers[0] = value;
    let cleanup = if convention == CallingConvention32::Cdecl {
        4
    } else {
        4 + 4 * count
    };
    next.registers[4] = next.registers[4].wrapping_add(cleanup);
    next.eip = RETURN;
    encode_state(&next, &mut expected.arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut expected.arena[56..96],
    )
    .unwrap();
    expected
}

fn returned(
    f: &mut Fixture,
    owner: Owner,
    token: u32,
    state: State32,
    value: u32,
    convention: CallingConvention32,
    count: u32,
) {
    let expected = expected_return(f, state, value, convention, count);
    assert_eq!(complete(f, owner, token), Ok(()));
    assert_eq!(saved(f), expected);
    assert_eq!(f.engine.guard(KEY, f.generation), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.caller), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.keeper), Ok(()));
}

fn check_last_error(f: &mut Fixture, owner: Owner) {
    let (token, state) = capture(f, owner, GET, CallingConvention32::Stdcall, 0, ESP, 0x402);
    returned(
        f,
        owner,
        token,
        state,
        LAST_ERROR,
        CallingConvention32::Stdcall,
        0,
    );
}

#[test]
fn current_process_rejects_module_symbol_and_numeric_aliases() {
    for module in [
        "kernel32",
        "kernel32.dll ",
        "./kernel32.dll",
        "ntdll.dll",
        "kernel32.dll\0",
    ] {
        assert_eq!(WindowsApi32::resolve(module, "GetCurrentProcess"), None);
    }
    for name in [
        "getcurrentprocess",
        "GETCURRENTPROCESS",
        "GetCurrentProcessA",
        "GetCurrentProcess\0",
        "#9",
    ] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", name), None);
    }
    for id in [0, 1, 0x0002_0000, 0x0001_ffff, u32::MAX] {
        assert_eq!(WindowsApi32::from_id(id), None);
    }
}

#[test]
fn current_process_zero_argument_returns_preserve_no_image_ram_last_error_and_all_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner);
        let current = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(CODE), 2)
            .unwrap();
        let stale = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(STALE), 1)
            .unwrap();
        upload(&mut f.engine, STALE, &[0x90]);
        for esp in [ESP, 0x8ffc, u32::MAX - 3] {
            for flags in [0x402, 0xcd7] {
                let (token, state) = capture(
                    &mut f,
                    owner,
                    CURRENT,
                    CallingConvention32::Stdcall,
                    0,
                    esp,
                    flags,
                );
                returned(
                    &mut f,
                    owner,
                    token,
                    state,
                    u32::MAX,
                    CallingConvention32::Stdcall,
                    0,
                );
                assert!(f.engine.memory().unwrap().is_code_current(&current));
                assert!(!f.engine.memory().unwrap().is_code_current(&stale));
                let before = saved(&f);
                assert_eq!(
                    complete(&mut f, owner, token),
                    Err(HostError::Call(CallError::InvalidToken))
                );
                assert_eq!(saved(&f), before);
            }
        }
        check_last_error(&mut f, owner);
    }
}

#[test]
fn current_process_wrong_saved_shapes_are_neutral_and_scalar_recoverable() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner);
        for (convention, count) in [
            (CallingConvention32::Cdecl, 0),
            (CallingConvention32::Thiscall, 0),
            (CallingConvention32::Stdcall, 1),
            (CallingConvention32::Stdcall, 16),
        ] {
            let (token, state) = capture(&mut f, owner, CURRENT, convention, count, ESP, 0xcd7);
            let before = saved(&f);
            for _ in 0..2 {
                assert_eq!(
                    complete(&mut f, owner, token),
                    Err(HostError::Call(CallError::InvalidRequest))
                );
                assert_eq!(saved(&f), before);
            }
            let expected = expected_return(&f, state, 0xfeed_face, convention, count);
            match owner {
                Owner::Replacement => f
                    .engine
                    .complete_call(KEY, f.generation, token, 0xfeed_face),
                Owner::Resident => {
                    f.engine
                        .complete_resident_call(KEY, f.caller, token, 0xfeed_face)
                }
            }
            .unwrap();
            assert_eq!(saved(&f), expected);
        }
        check_last_error(&mut f, owner);
    }
}

#[test]
fn current_process_success_never_revives_written_protected_or_remapped_code() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for mutation in 0..3 {
            let mut f = fixture(owner);
            let stale = f
                .engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(STALE), 1)
                .unwrap();
            assert_eq!(f.engine.guard_resident(KEY, f.stale), Ok(()));
            match mutation {
                0 => upload(&mut f.engine, STALE, &[0x90]),
                1 => f.engine.protect(STALE, 1, 7).unwrap(),
                _ => {
                    f.engine.unmap(STALE, 1).unwrap();
                    f.engine.map(STALE, 1, 7).unwrap();
                    upload(&mut f.engine, STALE, &[0x90]);
                }
            }
            let before_stale = f.engine.guard_resident(KEY, f.stale);
            assert!(before_stale.is_err());
            let (token, state) = capture(
                &mut f,
                owner,
                CURRENT,
                CallingConvention32::Stdcall,
                0,
                ESP,
                0xcd7,
            );
            returned(
                &mut f,
                owner,
                token,
                state,
                u32::MAX,
                CallingConvention32::Stdcall,
                0,
            );
            assert_eq!(f.engine.guard_resident(KEY, f.stale), before_stale);
            assert!(!f.engine.memory().unwrap().is_code_current(&stale));
            check_last_error(&mut f, owner);
        }
    }
}
