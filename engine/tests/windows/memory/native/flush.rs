use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, X87_OFFSET},
        x86::{encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{Access, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0xa538_1234_5678_abcd;
const CODE: u32 = 0x1000;
const ESP: u32 = 0x8080;
const RETURN: u32 = 0x9000;
const FLUSH: u32 = 0x0001_0008;

#[derive(Clone, Copy)]
enum Owner {
    Replacement,
    Resident,
}

#[test]
fn flush_instruction_cache_has_the_named_numeric_stdcall_three_word_contract() {
    for module in ["kernel32.dll", "KERNEL32.DLL", "KeRnEl32.dLl"] {
        let api = WindowsApi32::resolve(module, "FlushInstructionCache")
            .expect("the named instruction-cache provider is admitted");
        assert_eq!(api.id(), FLUSH);
        assert_eq!(WindowsApi32::from_id(FLUSH), Some(api));
        assert_eq!(api.convention(), CallingConvention32::Stdcall);
        assert_eq!(api.stack_words(), 3);
    }
}

#[test]
fn flush_instruction_cache_raw_id_completes_both_saved_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut engine = EngineInstance::new(2, KEY).unwrap();
        engine.map(CODE, 1, 7).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0x0f, 0x0b]);
        engine.upload(CODE, 2).unwrap();
        for (index, value) in [CODE, 2, CODE, FLUSH].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let unit = match owner {
            Owner::Replacement => u64::from(engine.compile_with_gates(1, 1).unwrap()),
            Owner::Resident => engine.compile_resident_with_gates(1, 1).unwrap().get(),
        };
        for (index, value) in [RETURN, u32::MAX, 0, 0].into_iter().enumerate() {
            engine.write32(ESP + index as u32 * 4, value).unwrap();
        }
        // native saved-stop input; the wasm fixture proves generated guest calls.
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
                reason: ExitReason::Gate { id: FLUSH },
            },
            &mut arena[56..96],
        )
        .unwrap();
        arena[96..100].fill(0);
        let call = match owner {
            Owner::Replacement => {
                engine.capture_call(KEY, unit as u32, CallingConvention32::Stdcall, 3)
            }
            Owner::Resident => {
                engine.capture_resident_call(KEY, unit, CallingConvention32::Stdcall, 3)
            }
        }
        .unwrap();
        assert_eq!(call.id, FLUSH);
        assert_eq!(&call.arguments[..3], &[u32::MAX, 0, 0]);
        let mut expected = engine.arena().to_vec();
        let mut next = state;
        next.registers[0] = 1;
        next.registers[4] = ESP + 16;
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
const TARGET: u32 = 0x10000;
const WINDOW: u32 = 0x1000_0000;
const TOP: u32 = 0xffff_f000;
const GET: u32 = 0x10001;
const SET: u32 = 0x10002;
const ALLOC: u32 = 0x10005;
const FREE: u32 = 0x10006;
const LAST_ERROR: u32 = 0x89ab_cdef;
const APIS: [u32; 5] = [FLUSH, GET, SET, ALLOC, FREE];
const PERMISSIONS: [u32; 6] = [0, 1, 3, 4, 5, 7];

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

fn describe(engine: &mut EngineInstance) {
    let fields = (0..APIS.len())
        .flat_map(|index| [CODE + index as u32 * 16, 2])
        .chain(
            APIS.into_iter()
                .enumerate()
                .flat_map(|(index, id)| [CODE + index as u32 * 16, id]),
        );
    for (index, value) in fields.enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture(owner: Owner) -> Fixture {
    let mut engine = EngineInstance::new(12, KEY).unwrap();
    for address in [CODE, KEEP] {
        engine.map(address, 1, 7).unwrap();
    }
    engine.map(0x8000, 1, 3).unwrap();
    for index in 0..APIS.len() {
        upload(&mut engine, CODE + index as u32 * 16, &[0x0f, 0x0b]);
    }
    upload(&mut engine, KEEP, &[0x90]);
    for (index, permission) in PERMISSIONS.into_iter().enumerate() {
        let address = TARGET + index as u32 * 4096;
        engine.map(address, 1, 3).unwrap();
        upload(&mut engine, address, &[0x30 + index as u8; 32]);
        engine.protect(address, 1, permission).unwrap();
    }
    engine.map(TOP, 1, 3).unwrap();
    upload(&mut engine, u32::MAX - 3, &[0x55, 0xa5, 0x5a, 0xaa]);
    engine.protect(TOP, 1, 1).unwrap();
    describe(&mut engine);
    let resident = engine.compile_resident_with_gates(5, 5).unwrap().get();
    for (index, value) in [KEEP, 1].into_iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let keep = engine.compile_resident(1).unwrap().get();
    describe(&mut engine);
    let generation = engine.compile_with_gates(5, 5).unwrap();
    let arena = engine.arena_mut().unwrap();
    arena[X87_OFFSET + 16..X87_OFFSET + 18].copy_from_slice(&0x027f_u16.to_le_bytes());
    arena[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x81a5_u16.to_le_bytes());
    for (index, byte) in arena[X87_OFFSET + 40..X87_OFFSET + 120]
        .iter_mut()
        .enumerate()
    {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(17);
    }
    let mut f = Fixture {
        engine,
        generation,
        resident,
        keep,
    };
    let (token, state) = capture(&mut f, owner, SET, &[LAST_ERROR]);
    returned(&mut f, owner, token, state, state.registers[0], 1);
    f
}

fn capture_with_shape(
    f: &mut Fixture,
    owner: Owner,
    id: u32,
    args: &[u32],
    convention: CallingConvention32,
    count: u32,
) -> (u32, State32) {
    f.engine.write32(ESP, RETURN).unwrap();
    for (index, value) in args.iter().copied().chain([0]).enumerate() {
        f.engine.write32(ESP + 4 + index as u32 * 4, value).unwrap();
    }
    let state = State32 {
        registers: [
            0x7654_3210,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            ESP,
            5,
            6,
            7,
        ],
        eip: CODE + APIS.iter().position(|&api| api == id).unwrap() as u32 * 16,
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
    let call = match owner {
        Owner::Replacement => f.engine.capture_call(KEY, f.generation, convention, count),
        Owner::Resident => f
            .engine
            .capture_resident_call(KEY, f.resident, convention, count),
    }
    .unwrap();
    (call.token, state)
}

fn capture(f: &mut Fixture, owner: Owner, id: u32, args: &[u32]) -> (u32, State32) {
    capture_with_shape(
        f,
        owner,
        id,
        args,
        CallingConvention32::Stdcall,
        args.len() as u32,
    )
}

fn complete(f: &mut Fixture, owner: Owner, token: u32) -> Result<(), HostError> {
    match owner {
        Owner::Replacement => f.engine.complete_windows_call(KEY, f.generation, token),
        Owner::Resident => f
            .engine
            .complete_resident_windows_call(KEY, f.resident, token),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    generation: u32,
    mapped_pages: u32,
    ram: Vec<Result<Vec<u8>, HostError>>,
    access: Vec<Result<ring3_engine::memory::BackingOffset, ring3_engine::memory::MemoryError>>,
    modules: Vec<(Vec<u8>, usize)>,
}

fn saved(f: &Fixture) -> Saved {
    let memory = f.engine.memory().unwrap();
    let addresses: Vec<u32> = [CODE, KEEP, 0x8000, TOP, WINDOW, 0x5000]
        .into_iter()
        .chain((0..6).map(|index| TARGET + index * 4096))
        .collect();
    let artifact = f.engine.artifact_bytes().unwrap();
    let caller = f.engine.resident_bytes(f.resident).unwrap();
    let keeper = f.engine.resident_bytes(f.keep).unwrap();
    Saved {
        arena: f.engine.arena().to_vec(),
        arena_pointer: f.engine.arena_address(),
        generation: f.engine.generation(),
        mapped_pages: memory.mapped_pages(),
        ram: addresses
            .iter()
            .map(|&address| {
                let mut bytes = vec![0; 4096];
                memory
                    .read(GuestAddress(address), &mut bytes)
                    .map_err(HostError::Memory)?;
                Ok(bytes)
            })
            .collect(),
        access: addresses
            .iter()
            .flat_map(|&address| {
                [Access::Read, Access::Write, Access::Execute]
                    .map(|access| memory.resolve(GuestAddress(address), access))
            })
            .collect(),
        modules: [artifact, caller, keeper]
            .into_iter()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
            .collect(),
    }
}

fn expected_return(f: &Fixture, state: State32, result: u32, count: u32) -> Saved {
    let mut expected = saved(f);
    let mut next = state;
    next.registers[0] = result;
    next.registers[4] += 4 + count * 4;
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

fn returned(f: &mut Fixture, owner: Owner, token: u32, state: State32, result: u32, count: u32) {
    let expected = expected_return(f, state, result, count);
    assert_eq!(complete(f, owner, token), Ok(()));
    assert_eq!(saved(f), expected);
    assert_eq!(f.engine.guard(KEY, f.generation), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.resident), Ok(()));
    assert_eq!(f.engine.guard_resident(KEY, f.keep), Ok(()));
}

fn window_returned(f: &mut Fixture, owner: Owner, token: u32, state: State32, allocating: bool) {
    let result = if allocating { WINDOW } else { 1 };
    let count = if allocating { 4 } else { 3 };
    let mut expected = expected_return(f, state, result, count);
    assert_eq!(complete(f, owner, token), Ok(()));
    let after = saved(f);
    expected.mapped_pages = if allocating {
        expected.mapped_pages + 1
    } else {
        expected.mapped_pages - 1
    };
    expected.ram[4] = if allocating {
        Ok(vec![0; 4096])
    } else {
        Err(HostError::Memory(ring3_engine::memory::MemoryError::Fault(
            ring3_engine::memory::MemoryFault {
                address: GuestAddress(WINDOW),
                access: Access::Read,
                reason: ring3_engine::memory::FaultReason::Unmapped,
            },
        )))
    };
    for access in [Access::Read, Access::Write, Access::Execute] {
        let resolved = f
            .engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(WINDOW), access);
        if allocating {
            assert!(resolved.is_ok());
        } else {
            assert!(
                matches!(resolved, Err(ring3_engine::memory::MemoryError::Fault(fault))
                if fault.address == GuestAddress(WINDOW) && fault.access == access
                    && fault.reason == ring3_engine::memory::FaultReason::Unmapped)
            );
        }
    }
    // backing-slot identity is not an allocation API promise; all other mappings stay exact.
    expected.access[12..15].copy_from_slice(&after.access[12..15]);
    assert_eq!(after, expected);
}

fn last_error(f: &mut Fixture, owner: Owner) {
    let (token, state) = capture(f, owner, GET, &[]);
    returned(f, owner, token, state, LAST_ERROR, 0);
}

#[test]
fn flush_instruction_cache_rejects_name_and_module_aliases() {
    for module in [
        "kernel32",
        "kernel32.dll ",
        "./kernel32.dll",
        "ntdll.dll",
        "kernel32.dll\0",
    ] {
        assert_eq!(WindowsApi32::resolve(module, "FlushInstructionCache"), None);
    }
    for name in [
        "flushinstructioncache",
        "FLUSHINSTRUCTIONCACHE",
        "FlushInstructionCacheA",
        "FlushInstructionCache\0",
        "#8",
    ] {
        assert_eq!(WindowsApi32::resolve("kernel32.dll", name), None);
    }
    for id in [0, 1, 0x0002_0000, 0x0001_ffff, u32::MAX] {
        assert_eq!(WindowsApi32::from_id(id), None);
    }
}

#[test]
fn flush_instruction_cache_valid_hints_preserve_permissions_ram_and_current_owners() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner);
        let snapshots = [
            CODE,
            KEEP,
            TARGET + 3 * 4096,
            TARGET + 4 * 4096,
            TARGET + 5 * 4096,
        ]
        .map(|address| {
            f.engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(address), 1)
                .unwrap()
        });
        let ranges = [
            (0, 0),
            (0, u32::MAX),
            (0x5000, 0),
            (0x5000, 1),
            (u32::MAX, 0),
            (u32::MAX, 1),
            (1, u32::MAX),
            (TARGET + 4095, 2),
            (CODE, 2),
            (KEEP, 1),
            (TOP, 4096),
            (0x5000, 0x10001),
        ]
        .into_iter()
        .chain((0..6).map(|index| (TARGET + index * 4096, 4096)));
        for (address, length) in ranges {
            let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, address, length]);
            returned(&mut f, owner, token, state, 1, 3);
            for snapshot in &snapshots {
                assert!(f.engine.memory().unwrap().is_code_current(snapshot));
            }
            let before = saved(&f);
            assert_eq!(
                complete(&mut f, owner, token),
                Err(HostError::Call(CallError::InvalidToken))
            );
            assert_eq!(saved(&f), before);
        }
        last_error(&mut f, owner);
    }
}

#[test]
fn flush_instruction_cache_invalid_private_arguments_and_shapes_remain_retryable() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner);
        for args in [
            [0, 0, 0],
            [1, 0, 0],
            [u32::MAX - 1, 0, 0],
            [u32::MAX, u32::MAX, 2],
            [u32::MAX, 0x8000_0000, 0x8000_0001],
        ] {
            let (token, state) = capture(&mut f, owner, FLUSH, &args);
            let before = saved(&f);
            for _ in 0..2 {
                assert_eq!(
                    complete(&mut f, owner, token),
                    Err(HostError::Call(CallError::InvalidRequest))
                );
                assert_eq!(saved(&f), before);
            }
            for (index, value) in [u32::MAX, 0, 0].into_iter().enumerate() {
                f.engine.write32(ESP + 4 + index as u32 * 4, value).unwrap();
            }
            let before = saved(&f);
            assert_eq!(
                complete(&mut f, owner, token),
                Err(HostError::Call(CallError::InvalidRequest))
            );
            assert_eq!(saved(&f), before);
            let expected = expected_return(&f, state, 0xfeed_face, 3);
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
            assert_eq!(saved(&f), expected);
        }
        for (convention, count) in [
            (CallingConvention32::Cdecl, 3),
            (CallingConvention32::Thiscall, 3),
            (CallingConvention32::Stdcall, 2),
            (CallingConvention32::Stdcall, 4),
        ] {
            let (token, _) =
                capture_with_shape(&mut f, owner, FLUSH, &[u32::MAX, 0, 0], convention, count);
            let before = saved(&f);
            assert_eq!(
                complete(&mut f, owner, token),
                Err(HostError::Call(CallError::InvalidRequest))
            );
            assert_eq!(saved(&f), before);
            f.engine.abandon_call(KEY, token).unwrap();
        }
        last_error(&mut f, owner);
    }
}

#[test]
fn flush_instruction_cache_never_revives_written_protected_or_freed_code() {
    for owner in [Owner::Replacement, Owner::Resident] {
        let mut f = fixture(owner);
        let (token, state) = capture(&mut f, owner, ALLOC, &[0, 4096, 0x3000, 0x40]);
        window_returned(&mut f, owner, token, state, true);
        upload(&mut f.engine, WINDOW, &[0x90, 0xeb, 0]);
        for (index, value) in [WINDOW, 3].into_iter().enumerate() {
            let at = TRANSFER_OFFSET + index * 4;
            f.engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let dynamic = f.engine.compile_resident(1).unwrap().get();
        let original = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(WINDOW), 3)
            .unwrap();
        let bytes = f.engine.resident_bytes(dynamic).unwrap();
        let retained = (bytes.to_vec(), bytes.as_ptr() as usize);
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 3]);
        returned(&mut f, owner, token, state, 1, 3);
        let bytes = f.engine.resident_bytes(dynamic).unwrap();
        assert_eq!((bytes.to_vec(), bytes.as_ptr() as usize), retained);
        assert!(f.engine.memory().unwrap().is_code_current(&original));

        upload(&mut f.engine, WINDOW, &[0x90, 0xeb, 0]);
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 3]);
        returned(&mut f, owner, token, state, 1, 3);
        assert!(!f.engine.memory().unwrap().is_code_current(&original));
        assert_eq!(
            f.engine.guard_resident(KEY, dynamic),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        let rewritten = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(WINDOW), 3)
            .unwrap();
        f.engine.protect(WINDOW, 1, 7).unwrap();
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 3]);
        returned(&mut f, owner, token, state, 1, 3);
        assert!(!f.engine.memory().unwrap().is_code_current(&rewritten));
        let same_mode = f
            .engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(WINDOW), 3)
            .unwrap();
        f.engine.protect(WINDOW, 1, 0).unwrap();
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, 0, u32::MAX]);
        returned(&mut f, owner, token, state, 1, 3);
        for snapshot in [&original, &rewritten, &same_mode] {
            assert!(!f.engine.memory().unwrap().is_code_current(snapshot));
        }
        f.engine.protect(WINDOW, 1, 7).unwrap();
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 3]);
        returned(&mut f, owner, token, state, 1, 3);
        for snapshot in [&original, &rewritten, &same_mode] {
            assert!(!f.engine.memory().unwrap().is_code_current(snapshot));
        }
        let (token, state) = capture(&mut f, owner, FREE, &[WINDOW, 0, 0x8000]);
        window_returned(&mut f, owner, token, state, false);
        let pages = f.engine.memory().unwrap().mapped_pages();
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 4096]);
        returned(&mut f, owner, token, state, 1, 3);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), pages);
        assert!(
            f.engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(WINDOW), Access::Read)
                .is_err()
        );
        let (token, state) = capture(&mut f, owner, ALLOC, &[0, 4096, 0x3000, 0x40]);
        window_returned(&mut f, owner, token, state, true);
        let (token, state) = capture(&mut f, owner, FLUSH, &[u32::MAX, WINDOW, 4096]);
        returned(&mut f, owner, token, state, 1, 3);
        let mut zeroes = [0xff; 4096];
        f.engine
            .memory()
            .unwrap()
            .read(GuestAddress(WINDOW), &mut zeroes)
            .unwrap();
        assert_eq!(zeroes, [0; 4096]);
        for snapshot in [&original, &rewritten, &same_mode] {
            assert!(!f.engine.memory().unwrap().is_code_current(snapshot));
        }
        assert_eq!(
            f.engine.guard_resident(KEY, dynamic),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        last_error(&mut f, owner);
    }
}
