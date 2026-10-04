use ring3_engine::{
    abi::{
        AbiError,
        arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
        x86::{
            EXIT_VERSION_4, decode_exit, decode_state, encode_exit, encode_exit_v2, encode_exit_v3,
            encode_exit_v4, encode_state,
        },
    },
    cpu::{ExecutionExit, ExitReason, InfrastructureFailure, dbt::RegistryError, x86::State32},
    loader::{self, LoadError},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

#[test]
fn numeric_exit_process_completion_accepts_the_saved_stdcall_frame() {
    let key = 0xe344_0000_0000_0001;
    let mut engine = EngineInstance::new(2, key).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    for (index, word) in [0x1000_u32, 2, 0x1000, 0x0001_0003].into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    engine.write32(0x8080, 0x1100).unwrap();
    engine.write32(0x8084, 0xf123_4567).unwrap();
    let mut state = State32::default();
    state.registers[0] = 0x1357_9bdf;
    state.registers[4] = 0x8080;
    state.eip = 0x1000;
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: 0x0001_0003 },
        },
        &mut arena[56..96],
    )
    .unwrap();
    let call = engine
        .capture_call(key, generation, CallingConvention32::Stdcall, 1)
        .unwrap();
    assert_eq!(
        engine.complete_windows_call(key, generation, call.token),
        Ok(())
    );
}

const KEY: u64 = 0xe344_0000_0000_0002;
const MAIN: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const STACK: u32 = 0x8000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x1700;
const EXIT: u32 = 0x0001_0003;
const GET: u32 = 0x0001_0001;
const SET: u32 = 0x0001_0002;
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
    let words = [
        base,
        2,
        base + 0x100,
        1,
        base + 0x200,
        2,
        base + 0x300,
        2,
        base + 0x400,
        2,
        base,
        EXIT,
        base + 0x200,
        CALLBACK_RETURN,
        base + 0x300,
        GET,
        base + 0x400,
        SET,
    ];
    for (index, word) in words.into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
}

fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(3, KEY).unwrap();
    for base in [MAIN, KEEP] {
        engine.map(base, 1, 7).unwrap();
        for offset in [0, 0x200, 0x300, 0x400] {
            upload(&mut engine, base + offset, &[0x0f, 0x0b]);
        }
        upload(&mut engine, base + 0x100, &[0x90]);
    }
    engine.map(STACK, 1, 3).unwrap();
    describe(&mut engine, MAIN);
    let resident = engine.compile_resident_with_gates(5, 4).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, resident, 0)
        .unwrap();
    describe(&mut engine, KEEP);
    let keep = engine.compile_resident_with_gates(5, 4).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, keep, 1)
        .unwrap();
    describe(&mut engine, MAIN);
    let generation = engine.compile_with_gates(5, 4).unwrap();
    Fixture {
        engine,
        generation,
        resident,
        keep,
    }
}

fn stop(engine: &mut EngineInstance, pc: u32, id: u32, esp: u32) {
    // native lifecycle input; the separate wasm proof owns real call/gate execution.
    let state = State32 {
        registers: [0x1357_9bdf, 3, 5, 7, esp, 11, 13, 17],
        eip: pc,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    arena[100..140].fill(0x5a);
    arena[TRANSFER_OFFSET..].fill(0xcc);
}

fn capture(
    f: &mut Fixture,
    owner: Owner,
    convention: CallingConvention32,
    words: u32,
    code: u32,
) -> u32 {
    f.engine.write32(ESP, RETURN).unwrap();
    for index in 0..16 {
        f.engine
            .write32(ESP + 4 + 4 * index, code.wrapping_add(index))
            .unwrap();
    }
    stop(&mut f.engine, MAIN, EXIT, ESP);
    match owner {
        Owner::Replacement => f.engine.capture_call(KEY, f.generation, convention, words),
        Owner::Resident => f
            .engine
            .capture_resident_call(KEY, f.resident, convention, words),
    }
    .unwrap()
    .token
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

fn scalar(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f
            .engine
            .complete_call(KEY, f.generation, token, 0xdead_beef),
        Owner::Resident => f
            .engine
            .complete_resident_call(KEY, f.resident, token, 0xdead_beef),
    }
}

fn terminal(code: u32, retired: u32) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3EX");
    bytes[4..8].copy_from_slice(&0x0001_0004_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&9_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&retired.to_le_bytes());
    bytes[24..28].copy_from_slice(&code.to_le_bytes());
    bytes
}

fn pages(f: &Fixture) -> Vec<Vec<u8>> {
    [MAIN, KEEP, STACK]
        .into_iter()
        .map(|address| {
            let mut bytes = vec![0; 4096];
            f.engine
                .memory()
                .unwrap()
                .read(GuestAddress(address), &mut bytes)
                .unwrap();
            bytes
        })
        .collect()
}

fn reject<T: std::fmt::Debug + PartialEq>(
    f: &mut Fixture,
    expected: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<T, HostError>,
) {
    let arena = f.engine.arena().to_vec();
    let ram = f.engine.memory().ok().map(|_| pages(f));
    assert_eq!(operation(f), Err(expected));
    assert_eq!(f.engine.arena(), arena);
    if let Some(ram) = ram {
        assert_eq!(pages(f), ram);
    }
}

#[test]
fn exit_process_has_a_closed_exact_name_id_and_stdcall_one_word_profile() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        assert_eq!(
            WindowsApi32::resolve(module, "ExitProcess"),
            Some(WindowsApi32::ExitProcess)
        );
    }
    assert_eq!(WindowsApi32::from_id(EXIT), Some(WindowsApi32::ExitProcess));
    assert_eq!(WindowsApi32::ExitProcess.id(), EXIT);
    assert_eq!(
        WindowsApi32::ExitProcess.convention(),
        CallingConvention32::Stdcall
    );
    assert_eq!(WindowsApi32::ExitProcess.stack_words(), 1);
    for module in [
        "kernel32",
        "kernel32.dll ",
        "kernel32.dll\0",
        "./kernel32.dll",
        "ntdll.dll",
        "kernel32.dℓℓ",
    ] {
        assert_eq!(WindowsApi32::resolve(module, "ExitProcess"), None);
    }
    for symbol in [
        "exitprocess",
        "EXITPROCESS",
        "ExitProcessA",
        "ExitProcess\0",
        "#1",
    ] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", symbol), None);
    }
    for id in [0, 1, GET - 1, EXIT + 2, u32::MAX] {
        assert_eq!(WindowsApi32::from_id(id), None);
    }
}

#[test]
fn terminal_v4_is_literal_full_u32_and_old_versions_refuse_without_partial_output() {
    assert_eq!(EXIT_VERSION_4, 4);
    for code in [0, 0x8000_0000, 0xf123_4567, u32::MAX] {
        let exit = ExecutionExit {
            retired: 0x1234_5678,
            reason: ExitReason::ProcessExited { code },
        };
        let expected = terminal(code, 0x1234_5678);
        let mut bytes = [0xa5; 40];
        encode_exit_v4(&exit, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(decode_exit(&expected), Ok(exit));
        for encode in [encode_exit, encode_exit_v2, encode_exit_v3] {
            let mut old = [0xa5; 40];
            assert_eq!(encode(&exit, &mut old), Err(AbiError::Exit));
            assert_eq!(old, [0xa5; 40]);
        }
        for version in [1_u16, 2, 3] {
            let mut old = expected;
            old[4..6].copy_from_slice(&version.to_le_bytes());
            assert_eq!(decode_exit(&old), Err(AbiError::Exit));
        }
        for offset in [12, 28, 32, 36] {
            let mut malformed = expected;
            malformed[offset] = 1;
            assert!(decode_exit(&malformed).is_err());
        }
        for (offset, value, error) in [
            (0, 0, AbiError::Magic),
            (4, 5, AbiError::Version),
            (6, 2, AbiError::Profile),
            (8, 39, AbiError::Length),
        ] {
            let mut malformed = expected;
            malformed[offset] = value;
            assert_eq!(decode_exit(&malformed), Err(error));
        }
        for length in [0, 39, 41] {
            let mut short = vec![0xa5; length];
            assert_eq!(encode_exit_v4(&exit, &mut short), Err(AbiError::Length));
            assert_eq!(short, vec![0xa5; length]);
            assert_eq!(decode_exit(&short), Err(AbiError::Length));
        }
    }
    for (reason, tag, detail) in [
        (ExitReason::Budget, 1_u32, 0),
        (ExitReason::Cancelled, 2, 0),
        (ExitReason::NeedCode, 3, 0),
        (ExitReason::CodeInvalidated, 6, 0),
        (
            ExitReason::Infrastructure(InfrastructureFailure::HelperRejected),
            7,
            3,
        ),
        (ExitReason::Gate { id: EXIT }, 8, EXIT),
    ] {
        let exit = ExecutionExit {
            retired: 17,
            reason,
        };
        let mut expected = terminal(detail, 17);
        expected[16..20].copy_from_slice(&tag.to_le_bytes());
        let mut bytes = [0xa5; 40];
        encode_exit_v4(&exit, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(decode_exit(&expected), Ok(exit));
        expected[4..6].copy_from_slice(&3_u16.to_le_bytes());
        encode_exit_v3(&exit, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
    for tag in [0_u32, 10, u32::MAX] {
        let mut bytes = terminal(0, 0);
        bytes[16..20].copy_from_slice(&tag.to_le_bytes());
        assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
    }
    let mut zero_gate = terminal(0, 0);
    zero_gate[16..20].copy_from_slice(&8_u32.to_le_bytes());
    assert_eq!(decode_exit(&zero_gate), Err(AbiError::Exit));
    let fault = ExecutionExit {
        retired: 2,
        reason: ExitReason::MemoryFault {
            fault: MemoryFault {
                address: GuestAddress(0xffff_fffe),
                access: Access::Read,
                reason: FaultReason::AddressOverflow,
            },
            length: 4,
        },
    };
    let mut bytes = [0; 40];
    encode_exit_v4(&fault, &mut bytes).unwrap();
    assert_eq!(decode_exit(&bytes), Ok(fault));
    bytes[36..40].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(decode_exit(&bytes), Err(AbiError::Exit));
}

#[test]
fn replacement_and_resident_publish_only_exit_and_use_the_private_saved_argument() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for code in [0, 0x8000_0000, 0xf123_4567, u32::MAX] {
            let mut f = fixture();
            let token = capture(&mut f, owner, CallingConvention32::Stdcall, 1, code);
            f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 112].fill(0x3d);
            f.engine.write32(ESP, 0xdead_beef).unwrap();
            f.engine.write32(ESP + 4, code ^ u32::MAX).unwrap();
            let address = f.engine.arena_address();
            let mut expected = f.engine.arena().to_vec();
            expected[56..96].copy_from_slice(&terminal(code, 0));
            assert_eq!(complete(&mut f, owner, token), Ok(()));
            assert_eq!(f.engine.arena(), expected);
            assert_eq!(f.engine.arena_address(), address);
            assert!(f.engine.is_open());
            assert_eq!(f.engine.key(), KEY);
            assert_eq!(f.engine.generation(), f.generation);
            assert_eq!(
                decode_exit(&f.engine.arena()[56..96]).unwrap(),
                ExecutionExit {
                    retired: 0,
                    reason: ExitReason::ProcessExited { code }
                }
            );
            let state = decode_state(&f.engine.arena()[..56]).unwrap();
            assert_eq!(state.eip, MAIN);
            assert_eq!(state.registers[4], ESP);
            assert_eq!(state.registers[0], 0x1357_9bdf);
            reject(&mut f, HostError::ProcessExited, |f| {
                complete(f, owner, token)
            });
        }
    }
}

#[test]
fn scalar_reserved_id_refusal_keeps_pending_for_provider_retry() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        let token = capture(&mut f, owner, CallingConvention32::Stdcall, 1, 0xf123_4567);
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            scalar(f, owner, token + 1)
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
        reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
            scalar(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            scalar(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
        reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
            scalar(f, owner, token)
        });
        assert_eq!(complete(&mut f, owner, token), Ok(()));
        assert_eq!(&f.engine.arena()[56..96], &terminal(0xf123_4567, 0));
    }
}

#[test]
fn provider_exact_shape_refusal_preserves_pending_and_cannot_scalar_return() {
    for owner in [Owner::Replacement, Owner::Resident] {
        for (convention, words) in [
            (CallingConvention32::Cdecl, 1),
            (CallingConvention32::Thiscall, 1),
            (CallingConvention32::Stdcall, 0),
            (CallingConvention32::Stdcall, 2),
            (CallingConvention32::Stdcall, 16),
        ] {
            let mut f = fixture();
            let token = capture(&mut f, owner, convention, words, 0xf123_4567);
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                complete(f, owner, token)
            });
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                scalar(f, owner, token)
            });
            assert_eq!(f.engine.abandon_call(KEY, token), Ok(()));
            let retry = capture(&mut f, owner, CallingConvention32::Stdcall, 1, u32::MAX);
            assert_eq!(complete(&mut f, owner, retry), Ok(()));
        }
    }
}

#[test]
fn full_owner_token_state_and_cancel_validation_precedes_terminal_publication() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        let token = capture(&mut f, owner, CallingConvention32::Stdcall, 1, 0xf123_4567);
        for bad_key in [KEY ^ 1, KEY ^ (1 << 40)] {
            reject(&mut f, HostError::InvalidArtifact, |f| match owner {
                Owner::Replacement => f.engine.complete_windows_call(bad_key, f.generation, token),
                Owner::Resident => f
                    .engine
                    .complete_resident_windows_call(bad_key, f.resident, token),
            });
        }
        for bad_token in [0, token + 1, u32::MAX] {
            reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
                complete(f, owner, bad_token)
            });
        }
        match owner {
            Owner::Replacement => {
                reject(&mut f, HostError::InvalidArtifact, |f| {
                    f.engine.complete_windows_call(KEY, f.generation + 1, token)
                });
                reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
                    f.engine
                        .complete_resident_windows_call(KEY, f.resident, token)
                });
            }
            Owner::Resident => {
                reject(
                    &mut f,
                    HostError::Resident(RegistryError::InvalidUnit),
                    |f| {
                        f.engine
                            .complete_resident_windows_call(KEY, f.resident ^ (1 << 32), token)
                    },
                );
                reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
                    f.engine.complete_resident_windows_call(KEY, f.keep, token)
                });
                reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
                    f.engine.complete_windows_call(KEY, f.generation, token)
                });
            }
        }
        for offset in [16, 32, 48, 56, 76, 80] {
            f.engine.arena_mut().unwrap()[offset] ^= 1;
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
            reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
                complete(f, owner, token)
            });
            f.engine.arena_mut().unwrap()[offset] ^= 1;
        }
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, token + 1)
        });
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
        assert_eq!(complete(&mut f, owner, token), Ok(()));
    }
}

#[test]
fn stale_owner_wins_before_token_cancel_and_keeps_pending_recoverable() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        let token = capture(&mut f, owner, CallingConvention32::Stdcall, 1, 7);
        f.engine.write32(MAIN, 0).unwrap();
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
        let error = match owner {
            Owner::Replacement => HostError::CodeInvalidated,
            Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
        };
        reject(&mut f, error, |f| complete(f, owner, token + 1));
        reject(&mut f, error, |f| complete(f, owner, token));
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
        assert_eq!(f.engine.abandon_call(KEY, token), Ok(()));
    }
}

fn deny_live_operations(f: &mut Fixture, error: HostError) {
    macro_rules! deny {
        ($method:ident($($argument:expr),*)) => {
            reject(f, error, |f| f.engine.$method($($argument),*).map(|_| ()));
        };
    }
    deny!(memory());
    deny!(arena_mut());
    deny!(artifact_bytes());
    deny!(resident_bytes(u64::MAX));
    deny!(dispatcher_bytes(0));
    deny!(map(1, u32::MAX, u32::MAX));
    deny!(protect(1, u32::MAX, u32::MAX));
    deny!(unmap(1, u32::MAX));
    deny!(upload(u32::MAX, u32::MAX));
    deny!(write32(u32::MAX, 1));
    deny!(write_words32(u32::MAX));
    deny!(store32(u32::MAX, 1));
    deny!(store_resident32(0, u64::MAX, u32::MAX, 1));
    deny!(compile(0));
    deny!(compile_with_gates(u32::MAX, u32::MAX));
    deny!(compile_entries(0, u32::MAX));
    deny!(compile_resident(0));
    deny!(compile_resident_with_gates(u32::MAX, u32::MAX));
    deny!(compile_resident_entries(0, u32::MAX));
    deny!(load_pe32(&[]));
    deny!(load_pe32_at(&[], 1));
    deny!(load_pe32_linked_at(&[], 1, 1));
    deny!(start_loaded_image(1, u32::MAX));
    deny!(guard(0, 0));
    deny!(guard_resident(0, u64::MAX));
    deny!(guard_dispatch_entry(0));
    deny!(lookup_resident(u32::MAX));
    deny!(lookup_installed_resident(0, u32::MAX));
    deny!(acknowledge_resident_installation(0, u64::MAX, u32::MAX));
    deny!(capture_call_raw(0, 0, 0, u32::MAX));
    deny!(capture_resident_call_raw(0, u64::MAX, 0, u32::MAX));
    deny!(capture_resident_callback_call_raw(
        0,
        u64::MAX,
        0,
        0,
        u32::MAX
    ));
    deny!(capture_active_resident_callback_call_raw(
        0,
        u64::MAX,
        0,
        0,
        u32::MAX
    ));
    deny!(complete_call(0, 0, 0, 1));
    deny!(complete_resident_call(0, u64::MAX, 0, 1));
    deny!(complete_resident_callback_call(0, u64::MAX, 0, 0, 1));
    deny!(complete_active_resident_callback_call(0, u64::MAX, 0, 0, 1));
    deny!(complete_windows_call(0, 0, 0));
    deny!(complete_resident_windows_call(0, u64::MAX, 0));
    deny!(abandon_call(0, 0));
    deny!(begin_callback(0, 0, 0, 0, 0, 0, &[]));
    deny!(begin_callback_from_transfer(0, 0, 0, 0, 0, 0, u32::MAX));
    deny!(finish_callback(0, 0, 0));
    deny!(abort_callback(0, 0));
    deny!(resume_callback_code(0, 0, 0, u32::MAX, u32::MAX));
    deny!(resume_callback_entries(0, 0, 0, u32::MAX, u32::MAX));
    deny!(begin_resident_callback(0, 0, 0, 0, 0, 0, 0, &[]));
    deny!(begin_resident_callback_from_transfer(
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        u32::MAX
    ));
    deny!(authorize_resident_callback(0, 0, 0));
    deny!(select_resident_callback_unit(0, 0, 0, 0));
    deny!(finish_resident_callback(0, 0, 0));
    deny!(compile_resident_callback_unit(0, 0, 0, u32::MAX, u32::MAX));
    deny!(acknowledge_resident_callback_installation(
        0,
        0,
        0,
        0,
        u32::MAX
    ));
}

#[test]
fn terminal_live_guard_precedes_all_malformed_inputs_and_close_changes_it_to_closed() {
    let mut f = fixture();
    let token = capture(&mut f, Owner::Resident, CallingConvention32::Stdcall, 1, 0);
    complete(&mut f, Owner::Resident, token).unwrap();
    let arena = f.engine.arena().to_vec();
    let address = f.engine.arena_address();
    deny_live_operations(&mut f, HostError::ProcessExited);
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(f.engine.arena_address(), address);
    assert!(f.engine.is_open());
    f.engine.close();
    assert!(!f.engine.is_open());
    assert_eq!(f.engine.generation(), 0);
    assert_eq!(f.engine.key(), KEY);
    assert_eq!(f.engine.arena_address(), address);
    assert_eq!(f.engine.arena(), arena);
    deny_live_operations(&mut f, HostError::Closed);
    for read in [
        EngineInstance::read8,
        EngineInstance::read16,
        EngineInstance::read32,
    ] {
        reject(&mut f, HostError::Closed, |f| read(&mut f.engine, ESP));
    }
    f.engine.close();
    assert_eq!(f.engine.arena(), arena);
}

fn helper(version: u32, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3MH");
    bytes[4..8].copy_from_slice(&(0x0001_0000 | version).to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

#[test]
fn postmortem_reads_update_only_the_existing_helper_with_success_and_fault_records() {
    let mut f = fixture();
    let token = capture(
        &mut f,
        Owner::Replacement,
        CallingConvention32::Stdcall,
        1,
        0xf123_4567,
    );
    complete(&mut f, Owner::Replacement, token).unwrap();
    for (read, address, expected) in [
        (
            EngineInstance::read8 as fn(&mut EngineInstance, u32) -> Result<(), HostError>,
            ESP + 4,
            helper(2, [0, 0x67, 0, 0, 0, 1]),
        ),
        (
            EngineInstance::read16,
            ESP + 4,
            helper(2, [0, 0x4567, 0, 0, 0, 2]),
        ),
        (
            EngineInstance::read32,
            ESP + 4,
            helper(1, [0, 0xf123_4567, 0, 0, 0, 0]),
        ),
        (EngineInstance::read8, 0, helper(2, [1, 0, 1, 0, 1, 1])),
        (
            EngineInstance::read16,
            u32::MAX,
            helper(2, [1, 0, 3, u32::MAX, 1, 2]),
        ),
        (
            EngineInstance::read32,
            u32::MAX,
            helper(1, [1, 0, 3, u32::MAX, 1, 4]),
        ),
    ] {
        let mut arena = f.engine.arena().to_vec();
        arena[100..140].copy_from_slice(&expected);
        assert_eq!(read(&mut f.engine, address), Ok(()));
        assert_eq!(f.engine.arena(), arena);
        reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.write32(ESP + 4, 0)
        });
    }
}

#[test]
fn forged_terminal_exit_bytes_do_not_terminate_a_fresh_private_context() {
    let mut f = fixture();
    f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&terminal(u32::MAX, 0));
    assert!(f.engine.memory().is_ok());
    assert_eq!(f.engine.guard(KEY, f.generation), Ok(()));
    assert_eq!(f.engine.guard_dispatch_entry(KEY), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.resident), Ok(()));
    assert_eq!(f.engine.write32(ESP, 7), Ok(()));
    assert_eq!(
        f.engine
            .capture_call(KEY, f.generation, CallingConvention32::Stdcall, 1),
        Err(HostError::Call(CallError::InvalidStop))
    );
    let token = capture(
        &mut f,
        Owner::Replacement,
        CallingConvention32::Stdcall,
        1,
        0,
    );
    assert_eq!(complete(&mut f, Owner::Replacement, token), Ok(()));
}

#[test]
fn replacement_callback_terminal_provider_is_busy_and_scalar_inner_cannot_bypass_it() {
    let mut f = fixture();
    let outer = capture(
        &mut f,
        Owner::Replacement,
        CallingConvention32::Stdcall,
        1,
        7,
    );
    let callback = f
        .engine
        .begin_callback(
            KEY,
            f.generation,
            outer,
            MAIN + 0x100,
            MAIN + 0x200,
            CALLBACK_RETURN,
            &[],
        )
        .unwrap();
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        complete(f, Owner::Replacement, outer)
    });
    f.engine.write32(0x8040, MAIN + 0x100).unwrap();
    f.engine.write32(0x8044, u32::MAX).unwrap();
    stop(&mut f.engine, MAIN, EXIT, 0x8040);
    let inner = f
        .engine
        .capture_call(KEY, f.generation, CallingConvention32::Stdcall, 1)
        .unwrap()
        .token;
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        complete(f, Owner::Replacement, inner + 1)
    });
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
    reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
        complete(f, Owner::Replacement, inner)
    });
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        complete(f, Owner::Replacement, inner)
    });
    reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
        scalar(f, Owner::Replacement, inner)
    });
    f.engine.abandon_call(KEY, inner).unwrap();
    f.engine.abort_callback(KEY, callback.token).unwrap();
    assert_eq!(complete(&mut f, Owner::Replacement, outer), Ok(()));
    assert_eq!(&f.engine.arena()[56..96], &terminal(7, 0));
}

#[test]
fn resident_home_and_active_callback_scalar_wrappers_share_reserved_id_refusal() {
    let mut f = fixture();
    let outer = capture(&mut f, Owner::Resident, CallingConvention32::Stdcall, 1, 7);
    let callback = f
        .engine
        .begin_resident_callback(
            KEY,
            f.resident,
            f.keep,
            outer,
            KEEP + 0x100,
            KEEP + 0x200,
            CALLBACK_RETURN,
            &[],
        )
        .unwrap();
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        complete(f, Owner::Resident, outer)
    });
    f.engine
        .authorize_resident_callback(KEY, f.keep, callback.token)
        .unwrap();
    f.engine.write32(0x8040, KEEP + 0x100).unwrap();
    f.engine.write32(0x8044, u32::MAX).unwrap();
    stop(&mut f.engine, KEEP, EXIT, 0x8040);
    let inner = f
        .engine
        .capture_resident_callback_call(
            KEY,
            f.keep,
            callback.token,
            CallingConvention32::Stdcall,
            1,
        )
        .unwrap()
        .token;
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        f.engine.complete_resident_windows_call(KEY, f.keep, inner)
    });
    reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.keep, callback.token, inner, 0xdead_beef)
    });
    reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
        f.engine.complete_active_resident_callback_call(
            KEY,
            f.keep,
            callback.token,
            inner,
            0xdead_beef,
        )
    });
    f.engine.abandon_call(KEY, inner).unwrap();
    f.engine.abort_callback(KEY, callback.token).unwrap();
    assert_eq!(complete(&mut f, Owner::Resident, outer), Ok(()));
    assert_eq!(&f.engine.arena()[56..96], &terminal(7, 0));
}

#[test]
fn linked_pe_exit_process_name_stays_unsupported_and_failure_atomic() {
    let mut bytes = pe::image();
    bytes[pe::DATA_RAW..].fill(0);
    pe::put32(&mut bytes, pe::OPTIONAL + 104, 0x3000);
    pe::put32(&mut bytes, pe::OPTIONAL + 108, 40);
    for (offset, value) in [
        (0, 0x3040),
        (12, 0x30c0),
        (16, 0x3080),
        (0x40, 0x3100),
        (0x80, 0x3100),
    ] {
        pe::put32(&mut bytes, pe::DATA_RAW + offset, value);
    }
    bytes[pe::DATA_RAW + 0xc0..pe::DATA_RAW + 0xcd].copy_from_slice(b"kernel32.dll\0");
    bytes[pe::DATA_RAW + 0x102..pe::DATA_RAW + 0x10e].copy_from_slice(b"ExitProcess\0");
    assert_eq!(
        loader::load_pe32_linked_at(&bytes, pe::BASE, 0x7000_0000, 5).unwrap_err(),
        LoadError::Unsupported
    );
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let arena = engine.arena().to_vec();
    let pointer = engine.arena_address();
    assert_eq!(
        engine.load_pe32_linked_at(&bytes, pe::BASE, 0x7000_0000),
        Err(HostError::Loader(LoadError::Unsupported))
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.arena_address(), pointer);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.generation(), 0);
    engine.load_pe32(&pe::image()).unwrap();
}
