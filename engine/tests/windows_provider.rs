use ring3_engine::{
    abi::{
        arena::{CANCEL_OFFSET, TRANSFER_OFFSET, TRANSFER_SIZE},
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0xe123_4567_89ab_cdef;
const MAIN: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const STACK: u32 = 0x8000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x9000;
const GET: u32 = 0x0001_0001;
const SET: u32 = 0x0001_0002;
const UNKNOWN: u32 = 17;
const CALLBACK_RETURN: u32 = 18;

#[derive(Clone, Copy)]
enum Owner {
    Replacement,
    Resident,
}

struct Fixture {
    engine: EngineInstance,
    generation: u32,
    resident: u64,
    keep: u64,
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn describe(engine: &mut EngineInstance, base: u32) {
    let pairs = [
        (base, 2),
        (base + 0x100, 2),
        (base + 0x200, 2),
        (base + 0x300, 1),
        (base + 0x400, 2),
        (base, GET),
        (base + 0x100, SET),
        (base + 0x200, UNKNOWN),
        (base + 0x400, CALLBACK_RETURN),
    ];
    for (index, (address, value)) in pairs.into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 8;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&address.to_le_bytes());
        engine.arena_mut().unwrap()[offset + 4..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture(key: u64) -> Fixture {
    let mut engine = EngineInstance::new(4, key).unwrap();
    for base in [MAIN, KEEP] {
        engine.map(base, 1, 7).unwrap();
        for offset in [0, 0x100, 0x200, 0x400] {
            upload(&mut engine, base + offset, &[0x0f, 0x0b]);
        }
        upload(&mut engine, base + 0x300, &[0x90]);
    }
    engine.map(STACK, 1, 3).unwrap();
    describe(&mut engine, MAIN);
    let resident = engine.compile_resident_with_gates(5, 4).unwrap().get();
    describe(&mut engine, KEEP);
    let keep = engine.compile_resident_with_gates(5, 4).unwrap().get();
    describe(&mut engine, MAIN);
    let generation = engine.compile_with_gates(5, 4).unwrap();
    Fixture {
        engine,
        generation,
        resident,
        keep,
    }
}

fn stop(engine: &mut EngineInstance, base: u32, id: u32, esp: u32) -> State32 {
    // Native lifecycle input only; the independent Wasm fixture supplies actual CALL/Gate execution.
    let state = State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: base
            + match id {
                GET => 0,
                SET => 0x100,
                UNKNOWN => 0x200,
                CALLBACK_RETURN => 0x400,
                _ => unreachable!(),
            },
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
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
    arena[100..140].fill(0x5a);
    arena[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0xcc);
    state
}

fn capture(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    convention: CallingConvention32,
    count: u32,
    argument: u32,
) -> (u32, State32) {
    f.engine.write32(ESP, RETURN).unwrap();
    for index in 0..16 {
        f.engine
            .write32(ESP + 4 + index * 4, argument.wrapping_add(index))
            .unwrap();
    }
    let state = stop(&mut f.engine, MAIN, id, ESP);
    let key = f.engine.key();
    let record = match owner {
        Owner::Replacement => f.engine.capture_call(key, f.generation, convention, count),
        Owner::Resident => f
            .engine
            .capture_resident_call(key, f.resident, convention, count),
    }
    .unwrap();
    (record.token, state)
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    let key = f.engine.key();
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(key, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(key, f.resident, token),
    }
}

fn generic(f: &mut Fixture, owner: Owner, token: u32) {
    let key = f.engine.key();
    match owner {
        Owner::Replacement => f
            .engine
            .complete_call(key, f.generation, token, 0xfeed_face),
        Owner::Resident => f
            .engine
            .complete_resident_call(key, f.resident, token, 0xfeed_face),
    }
    .unwrap();
}

fn call(error: CallError) -> HostError {
    HostError::Call(error)
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    key: u64,
    generation: u32,
    dispatcher: ModuleView,
    artifact: ModuleView,
    units: Vec<(u64, ModuleView)>,
    mapped_pages: Result<u32, HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

type ModuleView = Result<(Vec<u8>, usize), HostError>;

fn saved(f: &Fixture) -> Saved {
    let e = &f.engine;
    Saved {
        arena: e.arena().to_vec(),
        arena_pointer: e.arena_address(),
        key: e.key(),
        generation: e.generation(),
        dispatcher: e
            .dispatcher_bytes(e.key())
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        artifact: e
            .artifact_bytes()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        units: [f.resident, f.keep]
            .into_iter()
            .map(|id| {
                (
                    id,
                    e.resident_bytes(id)
                        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
                )
            })
            .collect(),
        mapped_pages: e.memory().map(|memory| memory.mapped_pages()),
        ram: [0, MAIN, KEEP, STACK, RETURN]
            .into_iter()
            .map(|address| {
                let mut page = vec![0; 4096];
                e.memory()?
                    .read(GuestAddress(address), &mut page)
                    .map_err(HostError::Memory)?;
                Ok(page)
            })
            .collect(),
    }
}

fn reject(
    f: &mut Fixture,
    expected: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<(), HostError>,
) {
    let before = saved(f);
    let snapshots =
        f.engine.memory().ok().map(|memory| {
            [MAIN, KEEP].map(|pc| memory.snapshot_code(GuestAddress(pc), 2).unwrap())
        });
    assert_eq!(operation(f), Err(expected));
    assert_eq!(saved(f), before);
    if let Some(snapshots) = snapshots {
        for snapshot in snapshots {
            assert!(f.engine.memory().unwrap().is_code_current(&snapshot));
        }
    }
}

fn completed(f: &mut Fixture, owner: Owner, token: u32, original: State32, eax: u32, cleanup: u32) {
    let mut expected = saved(f);
    expected.arena[16..20].copy_from_slice(&eax.to_le_bytes());
    expected.arena[32..36].copy_from_slice(&(original.registers[4] + cleanup).to_le_bytes());
    expected.arena[48..52].copy_from_slice(&RETURN.to_le_bytes());
    expected.arena[56..96].copy_from_slice(&[
        0x52, 0x33, 0x45, 0x58, 3, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    assert_eq!(complete(f, owner, token), Ok(()));
    assert_eq!(saved(f), expected);
}

fn assert_last_error(f: &mut Fixture, owner: Owner, value: u32) {
    let (token, state) = capture(f, owner, GET, CallingConvention32::Stdcall, 0, 0);
    completed(f, owner, token, state, value, 4);
}

#[test]
fn closed_names_ids_and_exact_stdcall_profile() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        for (name, api, id, count) in [
            ("GetLastError", WindowsApi32::GetLastError, GET, 0),
            ("SetLastError", WindowsApi32::SetLastError, SET, 1),
        ] {
            assert_eq!(WindowsApi32::resolve(module, name), Some(api));
            assert_eq!(WindowsApi32::from_id(id), Some(api));
            assert_eq!(api.id(), id);
            assert_eq!(api.convention(), CallingConvention32::Stdcall);
            assert_eq!(api.stack_words(), count);
        }
    }
    for module in [
        "",
        "kernel32",
        "kernel32.dll ",
        " kernel32.dll",
        "kernel32.dll\0",
        "kernel32.dll.exe",
        "C:\\Windows\\System32\\kernel32.dll",
        "./kernel32.dll",
        "kernel32.dℓℓ",
        "ntdll.dll",
    ] {
        assert_eq!(WindowsApi32::resolve(module, "GetLastError"), None);
    }
    for name in [
        "",
        "getlasterror",
        "GETLASTERROR",
        "SetLastErrorA",
        "GetLastError\0",
        "#1",
    ] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", name), None);
    }
    for id in [0, 1, UNKNOWN, GET - 1, SET + 4, u32::MAX] {
        assert_eq!(WindowsApi32::from_id(id), None);
    }
}

#[test]
fn replacement_and_resident_full_u32_roundtrip_void_and_exact_arena_publication() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(KEY);
        assert_last_error(&mut f, owner, 0);
        for value in [u32::MAX, 0x8000_0000, 0x1234_5678, 0] {
            let (token, state) =
                capture(&mut f, owner, SET, CallingConvention32::Stdcall, 1, value);
            completed(&mut f, owner, token, state, 0x89ab_cdef, 8);
            reject(&mut f, call(CallError::InvalidToken), |f| {
                complete(f, owner, token)
            });
            assert_last_error(&mut f, owner, value);
            assert_last_error(&mut f, owner, value);
        }
    }
}

#[test]
fn instance_isolation_and_fresh_context_initialization() {
    let mut first = fixture(KEY);
    let mut second = fixture(KEY ^ (1 << 40));
    let (token, state) = capture(
        &mut first,
        Owner::Replacement,
        SET,
        CallingConvention32::Stdcall,
        1,
        0xffff_fffc,
    );
    completed(&mut first, Owner::Replacement, token, state, 0x89ab_cdef, 8);
    assert_last_error(&mut second, Owner::Resident, 0);
    assert_last_error(&mut first, Owner::Resident, 0xffff_fffc);
    first.engine.close();
    let mut fresh = fixture(KEY);
    assert_last_error(&mut fresh, Owner::Replacement, 0);
    assert_last_error(&mut second, Owner::Replacement, 0);
}

#[test]
fn transfer_and_guest_stack_tamper_cannot_replace_private_captured_frame() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(KEY);
        let (token, state) = capture(
            &mut f,
            owner,
            SET,
            CallingConvention32::Stdcall,
            1,
            0xfedc_ba98,
        );
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE].fill(0x3d);
        f.engine.write32(ESP, 0xdead_beef).unwrap();
        f.engine.write32(ESP + 4, 0x1122_3344).unwrap();
        completed(&mut f, owner, token, state, 0x89ab_cdef, 8);
        assert_last_error(&mut f, owner, 0xfedc_ba98);
    }
}

#[test]
fn unknown_and_wrong_shape_preserve_pending_and_allow_old_scalar_recovery() {
    let shapes = [
        (UNKNOWN, CallingConvention32::Stdcall, 0),
        (GET, CallingConvention32::Stdcall, 1),
        (SET, CallingConvention32::Stdcall, 0),
        (SET, CallingConvention32::Stdcall, 2),
        (SET, CallingConvention32::Stdcall, 16),
        (SET, CallingConvention32::Cdecl, 1),
        (SET, CallingConvention32::Thiscall, 1),
    ];
    for owner in [Owner::Replacement, Owner::Resident] {
        for (id, convention, count) in shapes {
            let mut f = fixture(KEY);
            let (token, _) = capture(&mut f, owner, id, convention, count, 0xaabb_ccdd);
            reject(&mut f, call(CallError::InvalidRequest), |f| {
                complete(f, owner, token)
            });
            generic(&mut f, owner, token);
            assert_eq!(
                decode_state(&f.engine.arena()[..56]).unwrap().registers[0],
                0xfeed_face
            );
            assert_last_error(&mut f, owner, 0);
        }
    }
}

#[test]
fn full_key_owner_and_token_checks_precede_provider_selection() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(KEY);
        let (token, _) = capture(&mut f, owner, UNKNOWN, CallingConvention32::Cdecl, 0, 0);
        for bad_key in [KEY ^ 1, KEY ^ (1 << 40)] {
            reject(&mut f, HostError::InvalidArtifact, |f| match owner {
                Owner::Replacement => f.engine.complete_windows_call(bad_key, f.generation, token),
                Owner::Resident => f
                    .engine
                    .complete_resident_windows_call(bad_key, f.resident, token),
            });
        }
        for bad_token in [0, token + 1, u32::MAX] {
            reject(&mut f, call(CallError::InvalidToken), |f| {
                complete(f, owner, bad_token)
            });
        }
        match owner {
            Owner::Replacement => {
                for generation in [0, f.generation + 1] {
                    reject(&mut f, HostError::InvalidArtifact, |f| {
                        f.engine.complete_windows_call(KEY, generation, token)
                    });
                }
                reject(&mut f, call(CallError::InvalidToken), |f| {
                    f.engine
                        .complete_resident_windows_call(KEY, f.resident, token)
                });
            }
            Owner::Resident => {
                for unit in [0, f.resident ^ (1 << 32), u64::MAX] {
                    reject(
                        &mut f,
                        HostError::Resident(RegistryError::InvalidUnit),
                        |f| f.engine.complete_resident_windows_call(KEY, unit, token),
                    );
                }
                reject(&mut f, call(CallError::InvalidToken), |f| {
                    f.engine.complete_resident_windows_call(KEY, f.keep, token)
                });
                reject(&mut f, call(CallError::InvalidToken), |f| {
                    f.engine.complete_windows_call(KEY, f.generation, token)
                });
            }
        }
        generic(&mut f, owner, token);
        assert_last_error(&mut f, owner, 0);
    }
}

#[test]
fn state_exit_and_cancel_priority_preserve_effect_and_valid_retry() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for offset in [16, 48, 56, 76, 80] {
            let mut f = fixture(KEY);
            let (token, state) = capture(
                &mut f,
                owner,
                SET,
                CallingConvention32::Stdcall,
                1,
                0xffff_fffd,
            );
            let original = f.engine.arena()[offset];
            f.engine.arena_mut().unwrap()[offset] ^= 1;
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
            reject(&mut f, call(CallError::StateChanged), |f| {
                complete(f, owner, token)
            });
            f.engine.arena_mut().unwrap()[offset] = original;
            reject(&mut f, call(CallError::Cancelled), |f| {
                complete(f, owner, token)
            });
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
            if offset == 80 {
                // Observe private state after failed Set, then prove the ordinary retry path too.
                f.engine.abandon_call(KEY, token).unwrap();
                assert_last_error(&mut f, owner, 0);
                let (retry, state) = capture(
                    &mut f,
                    owner,
                    SET,
                    CallingConvention32::Stdcall,
                    1,
                    0xffff_fffd,
                );
                completed(&mut f, owner, retry, state, 0x89ab_cdef, 8);
            } else {
                completed(&mut f, owner, token, state, 0x89ab_cdef, 8);
            }
            assert_last_error(&mut f, owner, 0xffff_fffd);
        }
        for (id, convention, count) in [
            (UNKNOWN, CallingConvention32::Cdecl, 0),
            (SET, CallingConvention32::Stdcall, 1),
        ] {
            let mut f = fixture(KEY);
            let (token, _) = capture(&mut f, owner, id, convention, count, 0xaabb_ccdd);
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
            reject(&mut f, call(CallError::InvalidToken), |f| {
                complete(f, owner, token + 1)
            });
            reject(&mut f, call(CallError::Cancelled), |f| {
                complete(f, owner, token)
            });
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
            f.engine.abandon_call(KEY, token).unwrap();
            assert_last_error(&mut f, owner, 0);
        }
    }
}

#[test]
fn stale_code_precedes_pending_checks_and_does_not_apply_set() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(KEY);
        let (token, _) = capture(
            &mut f,
            owner,
            SET,
            CallingConvention32::Stdcall,
            1,
            0xffff_fffe,
        );
        f.engine.write32(MAIN, 0).unwrap();
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
        let error = match owner {
            Owner::Replacement => HostError::CodeInvalidated,
            Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
        };
        reject(&mut f, error, |f| complete(f, owner, token + 1));
        reject(&mut f, error, |f| complete(f, owner, token));
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
        f.engine.abandon_call(KEY, token).unwrap();
        upload(&mut f.engine, MAIN, &[0x0f, 0x0b]);
        describe(&mut f.engine, MAIN);
        match owner {
            Owner::Replacement => f.generation = f.engine.compile_with_gates(5, 4).unwrap(),
            Owner::Resident => {
                f.resident = f.engine.compile_resident_with_gates(5, 4).unwrap().get()
            }
        }
        assert_last_error(&mut f, owner, 0);
    }
}

#[test]
fn closed_first_preserves_tombstone_and_cannot_reopen_context() {
    let mut f = fixture(KEY);
    let (token, _) = capture(
        &mut f,
        Owner::Resident,
        SET,
        CallingConvention32::Stdcall,
        1,
        u32::MAX,
    );
    let arena = f.engine.arena().to_vec();
    f.engine.close();
    assert_eq!(f.engine.arena(), arena);
    for (key, generation, token) in [
        (KEY, f.generation, token),
        (0, 0, 0),
        (u64::MAX, u32::MAX, u32::MAX),
    ] {
        reject(&mut f, HostError::Closed, |f| {
            f.engine.complete_windows_call(key, generation, token)
        });
        reject(&mut f, HostError::Closed, |f| {
            f.engine
                .complete_resident_windows_call(key, u64::MAX, token)
        });
    }
    assert!(!f.engine.is_open());
    assert_eq!(f.engine.arena_mut(), Err(HostError::Closed));
}

#[test]
fn replacement_callback_inner_provider_is_busy_after_shared_validation() {
    let mut f = fixture(KEY);
    let (outer, _) = capture(
        &mut f,
        Owner::Replacement,
        SET,
        CallingConvention32::Stdcall,
        1,
        0xaabb_ccdd,
    );
    let callback = f
        .engine
        .begin_callback(
            KEY,
            f.generation,
            outer,
            MAIN + 0x300,
            MAIN + 0x400,
            CALLBACK_RETURN,
            &[],
        )
        .unwrap();
    reject(&mut f, call(CallError::Busy), |f| {
        complete(f, Owner::Replacement, outer)
    });
    f.engine.write32(0x8040, MAIN + 0x300).unwrap();
    f.engine.write32(0x8044, 0xffff_ffff).unwrap();
    stop(&mut f.engine, MAIN, SET, 0x8040);
    let inner = f
        .engine
        .capture_call(KEY, f.generation, CallingConvention32::Stdcall, 1)
        .unwrap();
    reject(&mut f, call(CallError::InvalidToken), |f| {
        complete(f, Owner::Replacement, inner.token + 1)
    });
    f.engine.arena_mut().unwrap()[16] ^= 1;
    reject(&mut f, call(CallError::StateChanged), |f| {
        complete(f, Owner::Replacement, inner.token)
    });
    f.engine.arena_mut().unwrap()[16] ^= 1;
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
    reject(&mut f, call(CallError::Cancelled), |f| {
        complete(f, Owner::Replacement, inner.token)
    });
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
    reject(&mut f, call(CallError::Busy), |f| {
        complete(f, Owner::Replacement, inner.token)
    });
    generic(&mut f, Owner::Replacement, inner.token);
    f.engine.abort_callback(KEY, callback.token).unwrap();
    generic(&mut f, Owner::Replacement, outer);
    assert_last_error(&mut f, Owner::Replacement, 0);
}

#[test]
fn resident_callback_owned_inner_and_suspended_outer_cannot_use_provider_wrappers() {
    let mut f = fixture(KEY);
    f.engine
        .acknowledge_resident_installation(KEY, f.resident, 0)
        .unwrap();
    f.engine
        .acknowledge_resident_installation(KEY, f.keep, 1)
        .unwrap();
    let (outer, _) = capture(
        &mut f,
        Owner::Resident,
        SET,
        CallingConvention32::Stdcall,
        1,
        0xaabb_ccdd,
    );
    let callback = f
        .engine
        .begin_resident_callback(
            KEY,
            f.resident,
            f.keep,
            outer,
            KEEP + 0x300,
            KEEP + 0x400,
            CALLBACK_RETURN,
            &[],
        )
        .unwrap();
    reject(&mut f, call(CallError::Busy), |f| {
        complete(f, Owner::Resident, outer)
    });
    f.engine
        .authorize_resident_callback(KEY, f.keep, callback.token)
        .unwrap();
    f.engine.write32(0x8040, KEEP + 0x300).unwrap();
    f.engine.write32(0x8044, u32::MAX).unwrap();
    stop(&mut f.engine, KEEP, SET, 0x8040);
    let inner = f
        .engine
        .capture_resident_callback_call(
            KEY,
            f.keep,
            callback.token,
            CallingConvention32::Stdcall,
            1,
        )
        .unwrap();
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_windows_call(KEY, f.keep, inner.token)
    });
    reject(&mut f, call(CallError::InvalidToken), |f| {
        f.engine
            .complete_windows_call(KEY, f.generation, inner.token)
    });
    f.engine
        .complete_resident_callback_call(KEY, f.keep, callback.token, inner.token, 0xfeed_face)
        .unwrap();
    f.engine.abort_callback(KEY, callback.token).unwrap();
    generic(&mut f, Owner::Resident, outer);
    assert_last_error(&mut f, Owner::Resident, 0);
}
