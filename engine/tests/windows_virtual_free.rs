use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const KEY: u64 = 0xa381_1234_5678_abcd;
const MAIN: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const STACK: u32 = 0x8000;
const ESP: u32 = STACK + 0x80;
const RETURN: u32 = 0x9000;
const WINDOW: u32 = 0x1000_0000;
const GET: u32 = 0x10001;
const ALLOC: u32 = 0x10005;
const FREE: u32 = 0x10006;

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

fn describe(engine: &mut EngineInstance) {
    for (index, value) in [
        MAIN,
        2,
        MAIN + 16,
        2,
        MAIN + 32,
        2,
        MAIN,
        GET,
        MAIN + 16,
        ALLOC,
        MAIN + 32,
        FREE,
    ]
    .into_iter()
    .enumerate()
    {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    engine.map(MAIN, 1, 7).unwrap();
    engine.map(KEEP, 1, 7).unwrap();
    engine.map(STACK, 1, 3).unwrap();
    for offset in [0, 16, 32] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        engine.upload(MAIN + offset, 2).unwrap();
    }
    engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
    engine.upload(KEEP, 1).unwrap();
    // all guest code is uploaded before any unit is published.
    describe(&mut engine);
    let resident = engine.compile_resident_with_gates(3, 3).unwrap().get();
    for (index, value) in [KEEP, 1].into_iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let keep = engine.compile_resident(1).unwrap().get();
    describe(&mut engine);
    let generation = engine.compile_with_gates(3, 3).unwrap();
    Fixture {
        engine,
        generation,
        resident,
        keep,
    }
}

fn capture(f: &mut Fixture, owner: Owner, id: u32, count: u32, args: [u32; 4]) -> (u32, State32) {
    capture_with_convention(f, owner, id, CallingConvention32::Stdcall, count, args)
}

fn capture_with_convention(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    convention: CallingConvention32,
    count: u32,
    args: [u32; 4],
) -> (u32, State32) {
    f.engine.write32(ESP, RETURN).unwrap();
    for (index, value) in args.into_iter().enumerate() {
        f.engine.write32(ESP + 4 + index as u32 * 4, value).unwrap();
    }
    // typed native gate input exercises ownership; no generated guest execution is claimed.
    let state = State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            ESP,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: MAIN
            + match id {
                GET => 0,
                ALLOC => 16,
                FREE => 32,
                _ => unreachable!(),
            },
        eflags: 0xcd7,
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
    arena[100..140].fill(0x5a);
    arena[140..].fill(0xcc);
    let record = match owner {
        Owner::Replacement => f.engine.capture_call(KEY, f.generation, convention, count),
        Owner::Resident => f
            .engine
            .capture_resident_call(KEY, f.resident, convention, count),
    }
    .unwrap();
    (record.token, state)
}

fn completed(f: &mut Fixture, owner: Owner, token: u32, state: State32, result: u32, count: u32) {
    let mut expected = f.engine.arena().to_vec();
    let mut next = state;
    next.registers[0] = result;
    next.registers[4] += 4 + count * 4;
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
    let actual = match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    };
    assert_eq!(actual, Ok(()));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap(), next);
    f.engine.guard(KEY, f.generation).unwrap();
    f.engine.guard_resident(KEY, f.resident).unwrap();
    f.engine.guard_resident(KEY, f.keep).unwrap();
}

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = vec![0; length];
    engine
        .memory()?
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(bytes)
}

fn last_error(f: &mut Fixture, owner: Owner) {
    let (token, state) = capture(f, owner, GET, 0, [0; 4]);
    completed(f, owner, token, state, 0, 0);
}

#[test]
fn virtual_free_releases_owned_pages_and_reuses_zeroed_first_fit() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        last_error(&mut f, owner);
        let (token, state) = capture(&mut f, owner, ALLOC, 4, [0, 4097, 0x3000, 4]);
        completed(&mut f, owner, token, state, WINDOW, 4);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 5);
        assert_eq!(ram(&f.engine, WINDOW, 8192).unwrap(), vec![0; 8192]);
        last_error(&mut f, owner);
        let mut expected_data = vec![0; 8192];
        for (offset, value) in [(0_u32, 0x11_u32), (4095, 0x22), (4096, 0x33), (8191, 0x44)] {
            f.engine.write8(WINDOW + offset, value).unwrap();
            expected_data[offset as usize] = value as u8;
        }
        assert_eq!(ram(&f.engine, WINDOW, 8192).unwrap(), expected_data);
        let (token, state) = capture(&mut f, owner, FREE, 3, [WINDOW, 0, 0x8000, 0]);
        let stack = ram(&f.engine, STACK, 4096).unwrap();
        let keeper = f.engine.resident_bytes(f.keep).unwrap();
        let keeper = (keeper.to_vec(), keeper.as_ptr() as usize);
        completed(&mut f, owner, token, state, 1, 3);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 3);
        assert_eq!(ram(&f.engine, STACK, 4096).unwrap(), stack);
        let retained = f.engine.resident_bytes(f.keep).unwrap();
        assert_eq!((retained.to_vec(), retained.as_ptr() as usize), keeper);
        for address in [WINDOW, WINDOW + 4096] {
            assert!(matches!(
                ram(&f.engine, address, 1),
                Err(HostError::Memory(MemoryError::Fault(fault)))
                    if fault.address == GuestAddress(address)
                        && fault.access == Access::Read
                        && fault.reason == FaultReason::Unmapped
            ));
        }
        last_error(&mut f, owner);
        let (token, state) = capture(&mut f, owner, ALLOC, 4, [0, 4097, 0x3000, 4]);
        completed(&mut f, owner, token, state, WINDOW, 4);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 5);
        assert_eq!(ram(&f.engine, WINDOW, 8192).unwrap(), vec![0; 8192]);
        last_error(&mut f, owner);
    }
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

fn retained(f: &Fixture) -> [(Vec<u8>, usize); 3] {
    let artifact = f.engine.artifact_bytes().unwrap();
    let resident = f.engine.resident_bytes(f.resident).unwrap();
    let keeper = f.engine.resident_bytes(f.keep).unwrap();
    [artifact, resident, keeper].map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
}

fn reject(
    f: &mut Fixture,
    error: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<(), HostError>,
) {
    let arena = f.engine.arena().to_vec();
    let pages = f.engine.memory().unwrap().mapped_pages();
    let data = ram(&f.engine, WINDOW, 8192).unwrap();
    let stack = ram(&f.engine, STACK, 4096).unwrap();
    let units = retained(f);
    let snapshots = [MAIN, KEEP].map(|address| {
        f.engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 1)
            .unwrap()
    });
    assert_eq!(operation(f), Err(error));
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(f.engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(ram(&f.engine, WINDOW, 8192).unwrap(), data);
    assert_eq!(ram(&f.engine, STACK, 4096).unwrap(), stack);
    assert_eq!(retained(f), units);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| f.engine.memory().unwrap().is_code_current(snapshot))
    );
}

fn last_error_value(f: &mut Fixture, owner: Owner, value: u32) {
    let (token, state) = capture(f, owner, GET, 0, [0; 4]);
    completed(f, owner, token, state, value, 0);
}

#[test]
fn virtual_free_has_the_exact_named_numeric_stdcall_contract() {
    use ring3_engine::windows::WindowsApi32;
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        assert_eq!(
            WindowsApi32::resolve(module, "VirtualFree"),
            Some(WindowsApi32::VirtualFree)
        );
    }
    for module in ["kernel32", "kernel32.dll ", "ntdll.dll", "./kernel32.dll"] {
        assert_eq!(WindowsApi32::resolve(module, "VirtualFree"), None);
    }
    for symbol in ["virtualfree", "VirtualFreeEx", "VirtualFree\0", "#6"] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", symbol), None);
    }
    assert_eq!(WindowsApi32::from_id(FREE), Some(WindowsApi32::VirtualFree));
    assert_eq!(WindowsApi32::VirtualFree.id(), FREE);
    assert_eq!(
        WindowsApi32::VirtualFree.convention(),
        CallingConvention32::Stdcall
    );
    assert_eq!(WindowsApi32::VirtualFree.stack_words(), 3);
}

#[test]
fn unknown_interior_foreign_and_released_addresses_are_consumed_false_returns() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        let (token, state) = capture(&mut f, owner, ALLOC, 4, [0, 4097, 0x3000, 4]);
        completed(&mut f, owner, token, state, WINDOW, 4);
        let foreign = WINDOW + 0x10000;
        f.engine.map(foreign, 1, 3).unwrap();
        f.engine.write32(foreign, 0x2468_ace0).unwrap();
        let data = ram(&f.engine, WINDOW, 8192).unwrap();
        let units = retained(&f);
        for address in [0, WINDOW + 1, WINDOW + 4096, MAIN, foreign] {
            let (token, state) = capture(&mut f, owner, FREE, 3, [address, 0, 0x8000, 0]);
            completed(&mut f, owner, token, state, 0, 3);
            assert_eq!(f.engine.memory().unwrap().mapped_pages(), 6);
            assert_eq!(ram(&f.engine, WINDOW, 8192).unwrap(), data);
            assert_eq!(
                ram(&f.engine, foreign, 4).unwrap(),
                0x2468_ace0_u32.to_le_bytes()
            );
            assert_eq!(retained(&f), units);
            assert_eq!(
                complete(&mut f, owner, token),
                Err(HostError::Call(CallError::InvalidToken))
            );
            last_error_value(&mut f, owner, 487);
        }
        let (token, state) = capture(&mut f, owner, FREE, 3, [WINDOW, 0, 0x8000, 0]);
        completed(&mut f, owner, token, state, 1, 3);
        last_error_value(&mut f, owner, 487);
        let (token, state) = capture(&mut f, owner, FREE, 3, [WINDOW, 0, 0x8000, 0]);
        completed(&mut f, owner, token, state, 0, 3);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 4);
        assert_eq!(
            ram(&f.engine, foreign, 4).unwrap(),
            0x2468_ace0_u32.to_le_bytes()
        );
        assert_eq!(retained(&f), units);
        last_error_value(&mut f, owner, 487);
    }
}

#[test]
fn unsupported_shapes_and_authority_guards_keep_release_retryable_and_arguments_frozen() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture();
        let (token, state) = capture(&mut f, owner, ALLOC, 4, [0, 4097, 0x3000, 4]);
        completed(&mut f, owner, token, state, WINDOW, 4);
        for (convention, count, args) in [
            (CallingConvention32::Stdcall, 3, [WINDOW, 1, 0x8000, 0]),
            (CallingConvention32::Stdcall, 3, [WINDOW, 0, 0x2000, 0]),
            (CallingConvention32::Stdcall, 3, [WINDOW, 0, 0x8001, 0]),
            (CallingConvention32::Cdecl, 3, [WINDOW, 0, 0x8000, 0]),
            (CallingConvention32::Stdcall, 2, [WINDOW, 0, 0x8000, 0]),
            (CallingConvention32::Stdcall, 4, [WINDOW, 0, 0x8000, 0]),
        ] {
            let (token, _) = capture_with_convention(&mut f, owner, FREE, convention, count, args);
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                complete(f, owner, token)
            });
            f.engine.abandon_call(KEY, token).unwrap();
        }
        last_error(&mut f, owner);
        let (token, state) = capture(&mut f, owner, FREE, 3, [WINDOW, 0, 0x8000, 0]);
        reject(&mut f, HostError::InvalidArtifact, |f| match owner {
            Owner::Replacement => f.engine.complete_windows_call(KEY ^ 1, f.generation, token),
            Owner::Resident => f
                .engine
                .complete_resident_windows_call(KEY ^ 1, f.resident, token),
        });
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, token + 1)
        });
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(
                f,
                match owner {
                    Owner::Replacement => Owner::Resident,
                    Owner::Resident => Owner::Replacement,
                },
                token,
            )
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        f.engine.arena_mut().unwrap()[96] = 1;
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, 0)
        });
        reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[96] = 0;
        f.engine.write32(ESP, u32::MAX).unwrap();
        f.engine.write32(ESP + 4, WINDOW + 0x10000).unwrap();
        f.engine.write32(ESP + 8, 1).unwrap();
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..].fill(0xa5);
        completed(&mut f, owner, token, state, 1, 3);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 3);
        assert!(ram(&f.engine, WINDOW, 1).is_err());
        last_error(&mut f, owner);
    }
}
