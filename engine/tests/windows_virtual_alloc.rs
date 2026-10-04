use ring3_engine::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError},
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0xa359_1234_5678_abcd;
const MAIN: u32 = 0x1000;
const KEEP: u32 = 0x3000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x9000;
const WINDOW: u32 = 0x1000_0000;
const GET: u32 = 0x10001;
const SET: u32 = 0x10002;
const ALLOC: u32 = 0x10005;

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

fn fixture(pages: u32) -> Fixture {
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    for base in [MAIN, KEEP] {
        engine.map(base, 1, 7).unwrap();
        for offset in [0, 16, 64] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
                .copy_from_slice(&[0x0f, 0x0b]);
            engine.upload(base + offset, 2).unwrap();
        }
    }
    engine.map(0x8000, 1, 3).unwrap();
    describe(&mut engine, MAIN);
    let resident = engine.compile_resident_with_gates(3, 3).unwrap().get();
    describe(&mut engine, KEEP);
    let keep = engine.compile_resident_with_gates(3, 3).unwrap().get();
    describe(&mut engine, MAIN);
    let generation = engine.compile_with_gates(3, 3).unwrap();
    Fixture {
        engine,
        generation,
        resident,
        keep,
    }
}

fn describe(engine: &mut EngineInstance, base: u32) {
    for (index, value) in [
        base,
        2,
        base + 16,
        2,
        base + 64,
        2,
        base,
        GET,
        base + 16,
        SET,
        base + 64,
        ALLOC,
    ]
    .into_iter()
    .enumerate()
    {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
}

fn capture(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    convention: CallingConvention32,
    count: u32,
    arguments: [u32; 4],
) -> (u32, State32) {
    f.engine.write32(ESP, RETURN).unwrap();
    for index in 0..16 {
        f.engine
            .write32(
                ESP + 4 + index * 4,
                arguments
                    .get(index as usize)
                    .copied()
                    .unwrap_or(0xface_cafe),
            )
            .unwrap();
    }
    // canonical native Gate input; the copied PE proves genuine CALL execution separately.
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
                SET => 16,
                ALLOC => 64,
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

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

fn ram(engine: &EngineInstance, address: u32, length: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = vec![0; length];
    engine
        .memory()?
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(bytes)
}

type ModuleView = Result<(Vec<u8>, usize), HostError>;
#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    artifact: ModuleView,
    units: [ModuleView; 2],
    pages: Result<u32, HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn saved(f: &Fixture) -> Saved {
    let e = &f.engine;
    Saved {
        arena: e.arena().to_vec(),
        arena_pointer: e.arena_address(),
        generation: e.generation(),
        artifact: e
            .artifact_bytes()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        units: [f.resident, f.keep].map(|id| {
            e.resident_bytes(id)
                .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
        }),
        pages: e.memory().map(|memory| memory.mapped_pages()),
        ram: [
            MAIN,
            KEEP,
            0x8000,
            WINDOW,
            WINDOW + 0x10000,
            WINDOW + 0x11000,
            WINDOW + 0x20000,
        ]
        .map(|address| ram(e, address, 4096))
        .to_vec(),
    }
}

fn reject(
    f: &mut Fixture,
    expected: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<(), HostError>,
) {
    let before = saved(f);
    let snapshots = [MAIN, KEEP].map(|address| {
        f.engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 2)
            .unwrap()
    });
    assert_eq!(operation(f), Err(expected));
    assert_eq!(saved(f), before);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| f.engine.memory().unwrap().is_code_current(snapshot))
    );
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
    complete(f, owner, token).unwrap();
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap(), next);
    f.engine.guard(KEY, f.generation).unwrap();
    f.engine.guard_resident(KEY, f.resident).unwrap();
    f.engine.guard_resident(KEY, f.keep).unwrap();
}

fn last_error(f: &mut Fixture, owner: Owner, expected: u32) {
    let (token, state) = capture(f, owner, GET, CallingConvention32::Stdcall, 0, [0; 4]);
    completed(f, owner, token, state, expected, 0);
}

fn set_error(f: &mut Fixture, owner: Owner, value: u32) {
    let (token, state) = capture(
        f,
        owner,
        SET,
        CallingConvention32::Stdcall,
        1,
        [value, 0, 0, 0],
    );
    completed(f, owner, token, state, state.registers[0], 1);
}

#[test]
fn virtual_alloc_has_a_named_windows_provider() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        assert_eq!(
            WindowsApi32::resolve(module, "VirtualAlloc"),
            Some(WindowsApi32::VirtualAlloc)
        );
    }
    for module in ["kernel32", "kernel32.dll ", "ntdll.dll", "./kernel32.dll"] {
        assert_eq!(WindowsApi32::resolve(module, "VirtualAlloc"), None);
    }
    for symbol in ["virtualalloc", "VirtualAllocEx", "VirtualAlloc\0", "#5"] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", symbol), None);
    }
    assert_eq!(
        WindowsApi32::from_id(ALLOC),
        Some(WindowsApi32::VirtualAlloc)
    );
    assert_eq!(WindowsApi32::VirtualAlloc.id(), ALLOC);
    assert_eq!(
        WindowsApi32::VirtualAlloc.convention(),
        CallingConvention32::Stdcall
    );
    assert_eq!(WindowsApi32::VirtualAlloc.stack_words(), 4);
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(32);
        set_error(&mut f, owner, 0xf123_4567);
        for (size, address, pages) in [
            (1, WINDOW, 1),
            (4097, WINDOW + 0x10000, 2),
            (65_536, WINDOW + 0x20000, 16),
        ] {
            let before = f.engine.memory().unwrap().mapped_pages();
            let (token, state) = capture(
                &mut f,
                owner,
                ALLOC,
                CallingConvention32::Stdcall,
                4,
                [0, size, 0x3000, 4],
            );
            completed(&mut f, owner, token, state, address, 4);
            assert_eq!(f.engine.memory().unwrap().mapped_pages(), before + pages);
            assert_eq!(
                ram(&f.engine, address, pages as usize * 4096).unwrap(),
                vec![0; pages as usize * 4096]
            );
            let fault = f
                .engine
                .memory()
                .unwrap()
                .fetch(GuestAddress(address), &mut [0; 1])
                .unwrap_err();
            assert!(
                matches!(fault, MemoryError::Fault(fault) if fault.address == GuestAddress(address) && fault.access == Access::Execute && fault.reason == FaultReason::Permission)
            );
            f.engine.write32(address + pages * 4096 - 4, size).unwrap();
            assert_eq!(
                ram(&f.engine, address + pages * 4096 - 4, 4).unwrap(),
                size.to_le_bytes()
            );
            assert!(ram(&f.engine, address + pages * 4096, 1).is_err());
            reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
                complete(f, owner, token)
            });
            last_error(&mut f, owner, 0xf123_4567);
        }
        assert_eq!(
            ram(&f.engine, WINDOW + 4092, 4).unwrap(),
            1_u32.to_le_bytes()
        );
        assert_eq!(
            ram(&f.engine, WINDOW + 0x11000 + 4092, 4).unwrap(),
            4097_u32.to_le_bytes()
        );
    }
}

#[test]
fn capacity_failure_is_an_ordinary_consumed_null_return_and_isolates_last_error() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(4);
        set_error(&mut f, owner, 0xf123_4567);
        let (token, state) = capture(
            &mut f,
            owner,
            ALLOC,
            CallingConvention32::Stdcall,
            4,
            [0, 4097, 0x3000, 4],
        );
        let before = saved(&f);
        completed(&mut f, owner, token, state, 0, 4);
        let after = saved(&f);
        assert_eq!(after.pages, before.pages);
        assert_eq!(after.ram, before.ram);
        assert_eq!(after.artifact, before.artifact);
        assert_eq!(after.units, before.units);
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, token)
        });
        last_error(&mut f, owner, 8);
        let (token, state) = capture(
            &mut f,
            owner,
            ALLOC,
            CallingConvention32::Stdcall,
            4,
            [0, 1, 0x3000, 4],
        );
        completed(&mut f, owner, token, state, WINDOW, 4);
        last_error(&mut f, owner, 8);
        let mut other = fixture(4);
        last_error(&mut other, owner, 0);
    }
}

#[test]
fn unsupported_requests_keep_the_captured_token_retryable_without_allocation() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(8);
        set_error(&mut f, owner, 0xf123_4567);
        for arguments in [
            [1, 1, 0x3000, 4],
            [0, 0, 0x3000, 4],
            [0, 65_537, 0x3000, 4],
            [0, u32::MAX, 0x3000, 4],
            [0, 1, 0x1000, 4],
            [0, 1, 0x2000, 4],
            [0, 1, 0x3001, 4],
            [0, 1, 0x3000, 0],
            [0, 1, 0x3000, 0x40],
        ] {
            let (token, _) = capture(
                &mut f,
                owner,
                ALLOC,
                CallingConvention32::Stdcall,
                4,
                arguments,
            );
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                complete(f, owner, token)
            });
            // generic recovery consumes the same pending record without invoking allocation.
            match owner {
                Owner::Replacement => f
                    .engine
                    .complete_call(KEY, f.generation, token, 0xfeed_face),
                Owner::Resident => {
                    f.engine
                        .complete_resident_call(KEY, f.resident, token, 0xfeed_face)
                }
            }
            .unwrap();
            assert_eq!(f.engine.memory().unwrap().mapped_pages(), 3);
            last_error(&mut f, owner, 0xf123_4567);
        }
        for (convention, count) in [
            (CallingConvention32::Cdecl, 4),
            (CallingConvention32::Thiscall, 4),
            (CallingConvention32::Stdcall, 3),
            (CallingConvention32::Stdcall, 5),
        ] {
            let (token, _) = capture(&mut f, owner, ALLOC, convention, count, [0, 1, 0x3000, 4]);
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                complete(f, owner, token)
            });
            f.engine.abandon_call(KEY, token).unwrap();
        }
        last_error(&mut f, owner, 0xf123_4567);
    }
}

#[test]
fn owner_token_state_and_cancel_priorities_precede_allocation_then_allow_retry() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(8);
        let (token, original) = capture(
            &mut f,
            owner,
            ALLOC,
            CallingConvention32::Stdcall,
            4,
            [0, 1, 0x3000, 4],
        );
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
        f.engine.arena_mut().unwrap()[96] = 1;
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, owner, 0)
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[16] ^= 1;
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            complete(f, owner, token)
        });
        f.engine.arena_mut().unwrap()[96] = 0;
        completed(&mut f, owner, token, original, WINDOW, 4);
        last_error(&mut f, owner, 0);
    }
}

#[test]
fn stale_code_and_closed_owners_preserve_pending_allocation_resources() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(8);
        let (token, _) = capture(
            &mut f,
            owner,
            ALLOC,
            CallingConvention32::Stdcall,
            4,
            [0, 1, 0x3000, 4],
        );
        f.engine.write8(MAIN, 0x0f).unwrap();
        let before = saved(&f);
        let expected = match owner {
            Owner::Replacement => HostError::CodeInvalidated,
            Owner::Resident => HostError::Resident(RegistryError::CodeInvalidated),
        };
        assert_eq!(complete(&mut f, owner, token), Err(expected));
        assert_eq!(saved(&f), before);
        f.engine.close();
        let arena = f.engine.arena().to_vec();
        assert_eq!(complete(&mut f, owner, token), Err(HostError::Closed));
        assert_eq!(f.engine.arena(), arena);
    }
}

#[test]
fn captured_arguments_are_owned_and_full_register_flags_arena_cleanup_remains_exact() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(8);
        let (token, state) = capture(
            &mut f,
            owner,
            ALLOC,
            CallingConvention32::Stdcall,
            4,
            [0, 1, 0x3000, 4],
        );
        f.engine.write32(ESP + 8, 65_536).unwrap();
        f.engine.write32(ESP, 0xffff_ffff).unwrap();
        f.engine.arena_mut().unwrap()[TRANSFER_OFFSET..].fill(0xa5);
        completed(&mut f, owner, token, state, WINDOW, 4);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 4);
        assert_eq!(ram(&f.engine, WINDOW, 4096).unwrap(), vec![0; 4096]);
        assert!(ram(&f.engine, WINDOW + 4096, 1).is_err());
        last_error(&mut f, owner, 0);
    }
}
