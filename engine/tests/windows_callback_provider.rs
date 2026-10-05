#[allow(dead_code)]
#[path = "support/pe32.rs"]
mod pe;

use ring3_engine::{
    abi::{
        arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, dbt::RegistryError, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{CallError, EngineInstance, HostError},
    windows::{CallingConvention32, WindowsApi32},
};

const KEY: u64 = 0xa360_1234_5678_abcd;
const OUTER: u32 = 0x4000;
const HOME: u32 = 0x5000;
const ACTIVE: u32 = 0x6000;
const OTHER: u32 = 0x3000;
const STACK: u32 = 0x8000;
const WINDOW: u32 = 0x1000_0000;
const SENTINEL: u32 = 0x89ab_cdef;
const LAST_ERROR: u32 = 0xf123_4567;
const APIS: [WindowsApi32; 5] = [
    WindowsApi32::GetLastError,
    WindowsApi32::SetLastError,
    WindowsApi32::ExitProcess,
    WindowsApi32::GetModuleHandleA,
    WindowsApi32::VirtualAlloc,
];
const ORIGINAL_PAGES: [u32; 9] = [
    OUTER,
    HOME,
    ACTIVE,
    OTHER,
    STACK,
    pe::BASE,
    pe::BASE + 0x1000,
    pe::BASE + 0x3000,
    pe::BASE + 0x5000,
];

struct Fixture {
    engine: EngineInstance,
    units: [u64; 4],
    generation: u32,
    callback_token: u32,
    active: u64,
    active_base: u32,
    outer_bytes: Vec<u8>,
    outer_state: State32,
}

fn descriptors(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    for (index, &(pc, value)) in blocks.iter().chain(gates).enumerate() {
        let at = TRANSFER_OFFSET + index * 8;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&pc.to_le_bytes());
        engine.arena_mut().unwrap()[at + 4..at + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn stop(engine: &mut EngineInstance, state: State32, reason: ExitReason) {
    let arena = engine.arena_mut().unwrap();
    // typed native states model ownership and effects; the copied pe supplies real guest calls.
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(&ExecutionExit { retired: 0, reason }, &mut arena[56..96]).unwrap();
}

fn base_fixture(pages: u32) -> Fixture {
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    engine.load_pe32(&pe::image()).unwrap();
    for address in [OUTER, HOME, ACTIVE, OTHER] {
        engine.map(address, 1, 7).unwrap();
    }
    engine.map(STACK, 1, 3).unwrap();
    for (address, bytes) in [(OUTER, &[0x0f, 0x0b][..]), (OTHER, &[0x90][..])] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(address, bytes.len() as u32).unwrap();
    }
    for base in [HOME, ACTIVE] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
        engine.upload(base, 1).unwrap();
        for offset in [0x100, 0x110, 0x120, 0x130, 0x140, 0x200] {
            engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
                .copy_from_slice(&[0x0f, 0x0b]);
            engine.upload(base + offset, 2).unwrap();
        }
    }
    let mut units = [0; 4];
    descriptors(&mut engine, &[(OUTER, 2)], &[(OUTER, 17)]);
    units[0] = engine.compile_resident_with_gates(1, 1).unwrap().get();
    for (index, base) in [HOME, ACTIVE].into_iter().enumerate() {
        let mut blocks = vec![(base, 1)];
        let mut gates = Vec::new();
        for (slot, api) in APIS.into_iter().enumerate() {
            blocks.push((base + 0x100 + slot as u32 * 16, 2));
            gates.push((base + 0x100 + slot as u32 * 16, api.id()));
        }
        blocks.push((base + 0x200, 2));
        gates.push((base + 0x200, 18));
        descriptors(&mut engine, &blocks, &gates);
        units[index + 1] = engine.compile_resident_with_gates(7, 6).unwrap().get();
        engine
            .acknowledge_resident_installation(KEY, units[index + 1], index as u32 + 1)
            .unwrap();
    }
    descriptors(&mut engine, &[(OTHER, 1)], &[]);
    units[3] = engine.compile_resident(1).unwrap().get();
    let mut blocks = vec![(OUTER, 2), (HOME, 1)];
    let mut gates = vec![(OUTER, 17)];
    for (slot, api) in APIS.into_iter().enumerate() {
        blocks.push((HOME + 0x100 + slot as u32 * 16, 2));
        gates.push((HOME + 0x100 + slot as u32 * 16, api.id()));
    }
    blocks.push((HOME + 0x200, 2));
    gates.push((HOME + 0x200, 18));
    descriptors(&mut engine, &blocks, &gates);
    let generation = engine.compile_with_gates(8, 7).unwrap();
    engine.write32(0x8ffc, 0x3005).unwrap();
    let outer_state = State32 {
        registers: [10, 0x1357_9bdf, 3, 4, 0x8ffc, 6, 7, 8],
        eip: OUTER,
        eflags: 0xcd7,
    };
    stop(&mut engine, outer_state, ExitReason::Gate { id: 17 });
    let outer_bytes = engine.arena()[..96].to_vec();
    Fixture {
        engine,
        units,
        generation,
        callback_token: 0,
        active: units[1],
        active_base: HOME,
        outer_bytes,
        outer_state,
    }
}

fn fixture(pages: u32, foreign: bool, armed: bool) -> Fixture {
    let mut f = base_fixture(pages);
    let outer = f
        .engine
        .capture_resident_call(KEY, f.units[0], CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(outer.token, 1);
    let callback = f
        .engine
        .begin_resident_callback(KEY, f.units[0], f.units[1], 1, HOME, HOME + 0x200, 18, &[])
        .unwrap();
    f.callback_token = callback.token;
    assert_eq!((callback.token, callback.entry_esp), (2, 0x8ff8));
    if armed {
        f.engine
            .authorize_resident_callback(KEY, f.units[1], callback.token)
            .unwrap();
        if foreign {
            let mut state = decode_state(&f.engine.arena()[..56]).unwrap();
            state.eip = ACTIVE;
            stop(&mut f.engine, state, ExitReason::NeedCode);
            let before = f.engine.arena().to_vec();
            f.engine
                .select_resident_callback_unit(KEY, f.units[1], callback.token, f.units[2])
                .unwrap();
            assert_eq!(f.engine.arena(), before);
            f.active = f.units[2];
            f.active_base = ACTIVE;
        }
    }
    f
}

fn capture(
    f: &mut Fixture,
    api: WindowsApi32,
    convention: CallingConvention32,
    arguments: &[u32],
) -> (u32, State32) {
    let esp = 0x8ff8 - 4 * (arguments.len() as u32 + 1);
    f.engine.write32(esp, f.active_base + 1).unwrap();
    for (index, argument) in arguments.iter().enumerate() {
        f.engine
            .write32(esp + 4 * (index as u32 + 1), *argument)
            .unwrap();
    }
    let slot = APIS.iter().position(|&value| value == api).unwrap() as u32;
    let state = State32 {
        registers: [
            SENTINEL,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: f.active_base + 0x100 + slot * 16,
        eflags: 0xcd7,
    };
    stop(&mut f.engine, state, ExitReason::Gate { id: api.id() });
    f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
    f.engine.arena_mut().unwrap()[100..140].fill(0x5a);
    let call = f
        .engine
        .capture_active_resident_callback_call(
            KEY,
            f.active,
            f.callback_token,
            convention,
            arguments.len() as u32,
        )
        .unwrap();
    assert_eq!(
        (call.id, call.entry_esp, call.return_pc),
        (api.id(), esp, f.active_base + 1)
    );
    (call.token, state)
}

fn complete(f: &mut Fixture, token: u32) -> Result<(), HostError> {
    f.engine
        .complete_active_resident_callback_windows_call(KEY, f.active, f.callback_token, token)
}

fn ram(engine: &EngineInstance, address: u32) -> Result<Vec<u8>, HostError> {
    let mut bytes = vec![0; 4096];
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
    pointer: usize,
    generation: u32,
    artifact: ModuleView,
    units: [ModuleView; 4],
    pages: Result<u32, HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
}

fn saved(f: &Fixture) -> Saved {
    let engine = &f.engine;
    Saved {
        arena: engine.arena().to_vec(),
        pointer: engine.arena_address(),
        generation: engine.generation(),
        artifact: engine
            .artifact_bytes()
            .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize)),
        units: f.units.map(|id| {
            engine
                .resident_bytes(id)
                .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize))
        }),
        pages: engine.memory().map(|memory| memory.mapped_pages()),
        ram: ORIGINAL_PAGES
            .into_iter()
            .chain([
                WINDOW,
                WINDOW + 4096,
                WINDOW + 0x10000,
                WINDOW + 0x11000,
                WINDOW + 0x20000,
            ])
            .map(|address| ram(engine, address))
            .collect(),
    }
}

fn reject(
    f: &mut Fixture,
    error: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<(), HostError>,
) {
    let before = saved(f);
    assert_eq!(operation(f), Err(error));
    assert_eq!(saved(f), before);
}

fn completed(f: &mut Fixture, token: u32, state: State32, result: u32) {
    let before = saved(f);
    let old_ram = ORIGINAL_PAGES.map(|address| ram(&f.engine, address));
    let mut next = state;
    next.registers[0] = result;
    next.registers[4] = 0x8ff8;
    next.eip = f.active_base + 1;
    let mut expected = before.arena.clone();
    encode_state(&next, &mut expected[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut expected[56..96],
    )
    .unwrap();
    assert_eq!(complete(f, token), Ok(()));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(
        ORIGINAL_PAGES.map(|address| ram(&f.engine, address)),
        old_ram
    );
    let after = saved(f);
    assert_eq!(
        (after.units, after.artifact, after.pointer, after.generation),
        (
            before.units,
            before.artifact,
            before.pointer,
            before.generation
        )
    );
    assert_eq!(f.engine.guard_resident(KEY, f.active), Ok(()));
}

fn invoke(f: &mut Fixture, api: WindowsApi32, arguments: &[u32], result: u32) -> u32 {
    let (token, state) = capture(f, api, CallingConvention32::Stdcall, arguments);
    completed(f, token, state, result);
    token
}

fn finish(f: &mut Fixture, result: u32) {
    let mut state = decode_state(&f.engine.arena()[..56]).unwrap();
    state.eip = HOME + 0x200;
    state.registers[0] = result;
    state.registers[4] = 0x8ffc;
    if f.active != f.units[1] {
        stop(&mut f.engine, state, ExitReason::NeedCode);
        f.engine
            .select_resident_callback_unit(KEY, f.units[1], f.callback_token, f.units[1])
            .unwrap();
    }
    stop(&mut f.engine, state, ExitReason::Gate { id: 18 });
    let receipt = f
        .engine
        .finish_resident_callback(KEY, f.units[1], f.callback_token)
        .unwrap();
    assert_eq!(
        (receipt.token, receipt.outer_token, receipt.result),
        (f.callback_token, 1, result)
    );
    assert_eq!(&f.engine.arena()[..96], f.outer_bytes);
    f.engine
        .complete_resident_call(KEY, f.units[0], 1, result)
        .unwrap();
    let mut expected = f.outer_state;
    expected.registers[0] = result;
    expected.registers[4] = 0x9000;
    expected.eip = 0x3005;
    assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap(), expected);
}

#[test]
fn returning_windows_provider_completes_a_callback_owned_call() {
    for foreign in [false, true] {
        let mut f = fixture(16, foreign, true);
        assert_eq!(invoke(&mut f, WindowsApi32::GetLastError, &[], 0), 3);
        assert_eq!(
            invoke(&mut f, WindowsApi32::SetLastError, &[LAST_ERROR], SENTINEL),
            4
        );
        assert_eq!(
            invoke(&mut f, WindowsApi32::GetLastError, &[], LAST_ERROR),
            5
        );
        invoke(&mut f, WindowsApi32::GetModuleHandleA, &[0], pe::BASE);
        invoke(
            &mut f,
            WindowsApi32::VirtualAlloc,
            &[0, 4097, 0x3000, 4],
            WINDOW,
        );
        invoke(&mut f, WindowsApi32::GetLastError, &[], LAST_ERROR);
        assert_eq!(ram(&f.engine, WINDOW).unwrap(), vec![0; 4096]);
        assert_eq!(ram(&f.engine, WINDOW + 4096).unwrap(), vec![0; 4096]);
        finish(&mut f, 37);
    }
}

#[test]
fn allocation_rounding_zero_rw_and_capacity_failure_are_callback_continuations() {
    let mut f = fixture(12, true, true);
    invoke(&mut f, WindowsApi32::SetLastError, &[LAST_ERROR], SENTINEL);
    for (size, address, pages) in [(1, WINDOW, 1), (4097, WINDOW + 0x10000, 2)] {
        invoke(
            &mut f,
            WindowsApi32::VirtualAlloc,
            &[0, size, 0x3000, 4],
            address,
        );
        for page in 0..pages {
            let at = address + page * 4096;
            assert_eq!(ram(&f.engine, at).unwrap(), vec![0; 4096]);
            assert_eq!(
                f.engine
                    .memory()
                    .unwrap()
                    .fetch(GuestAddress(at), &mut [0; 1]),
                Err(MemoryError::Fault(MemoryFault {
                    address: GuestAddress(at),
                    access: Access::Execute,
                    reason: FaultReason::Permission
                }))
            );
            f.engine.write32(at, 0x1122_3344).unwrap();
            f.engine.write32(at + 4092, 0x5566_7788).unwrap();
        }
        invoke(&mut f, WindowsApi32::GetLastError, &[], LAST_ERROR);
    }
    let before = saved(&f);
    invoke(
        &mut f,
        WindowsApi32::VirtualAlloc,
        &[0, 65536, 0x3000, 4],
        0,
    );
    assert_eq!(f.engine.memory().unwrap().mapped_pages(), 12);
    assert_eq!(
        saved(&f).ram[ORIGINAL_PAGES.len()..],
        before.ram[ORIGINAL_PAGES.len()..]
    );
    invoke(&mut f, WindowsApi32::GetLastError, &[], 8);
    finish(&mut f, 8);
}

#[test]
fn exact_active_callback_and_inner_tokens_keep_priorities_and_old_routes_refuse() {
    let mut f = fixture(12, true, true);
    let (inner, state) = capture(
        &mut f,
        WindowsApi32::GetLastError,
        CallingConvention32::Stdcall,
        &[],
    );
    for (key, unit, callback, token, error) in [
        (KEY ^ 1, f.active, 0, 0, HostError::InvalidArtifact),
        (
            KEY,
            f.active ^ (1_u64 << 32),
            0,
            0,
            HostError::Resident(RegistryError::InvalidUnit),
        ),
        (KEY, f.units[1], 2, inner, HostError::Call(CallError::Busy)),
        (KEY, f.units[3], 2, inner, HostError::Call(CallError::Busy)),
        (
            KEY,
            f.active,
            0,
            inner,
            HostError::Call(CallError::InvalidToken),
        ),
        (
            KEY,
            f.active,
            inner,
            inner,
            HostError::Call(CallError::InvalidToken),
        ),
        (
            KEY,
            f.active,
            2,
            0,
            HostError::Call(CallError::InvalidToken),
        ),
        (
            KEY,
            f.active,
            2,
            2,
            HostError::Call(CallError::InvalidToken),
        ),
        (KEY, f.active, 2, 1, HostError::Call(CallError::Busy)),
    ] {
        reject(&mut f, error, |f| {
            f.engine
                .complete_active_resident_callback_windows_call(key, unit, callback, token)
        });
    }
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        f.engine
            .complete_resident_windows_call(KEY, f.active, inner)
    });
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        f.engine.complete_windows_call(KEY, f.generation, inner)
    });
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        f.engine.complete_resident_call(KEY, f.active, inner, 44)
    });
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        f.engine
            .complete_resident_callback_call(KEY, f.units[1], 2, inner, 44)
    });
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        f.engine.complete_resident_windows_call(KEY, f.units[0], 1)
    });
    completed(&mut f, inner, state, 0);
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        complete(f, inner)
    });
    assert_eq!(
        invoke(&mut f, WindowsApi32::GetLastError, &[], 0),
        inner + 1
    );
    finish(&mut f, 11);
}

#[test]
fn frozen_state_cancel_and_closed_errors_are_atomic_and_valid_pending_can_retry() {
    for offset in [0, 52, 56, 76] {
        let mut f = fixture(12, false, true);
        let (inner, state) = capture(
            &mut f,
            WindowsApi32::GetLastError,
            CallingConvention32::Stdcall,
            &[],
        );
        let frozen = f.engine.arena()[..96].to_vec();
        f.engine.arena_mut().unwrap()[offset] ^= 1;
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
            complete(f, inner)
        });
        f.engine.arena_mut().unwrap()[..96].copy_from_slice(&frozen);
        reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
            complete(f, inner)
        });
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
        completed(&mut f, inner, state, 0);
    }
    for foreign in [false, true] {
        let mut f = fixture(12, foreign, true);
        let (inner, _) = capture(
            &mut f,
            WindowsApi32::GetLastError,
            CallingConvention32::Stdcall,
            &[],
        );
        f.engine.close();
        reject(&mut f, HostError::Closed, |f| {
            f.engine
                .complete_active_resident_callback_windows_call(KEY ^ 1, 0, 0, inner)
        });
    }
}

#[test]
fn stale_active_home_and_outer_currency_precedes_inner_token_and_allows_neutral_abort() {
    for address in [OUTER, HOME, ACTIVE] {
        let mut f = fixture(12, true, true);
        let (inner, _) = capture(
            &mut f,
            WindowsApi32::GetLastError,
            CallingConvention32::Stdcall,
            &[],
        );
        f.engine.write32(address, 0x9090_9090).unwrap();
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
            .copy_from_slice(&1_u32.to_le_bytes());
        reject(
            &mut f,
            HostError::Resident(RegistryError::CodeInvalidated),
            |f| {
                f.engine
                    .complete_active_resident_callback_windows_call(KEY, f.active, 2, 0)
            },
        );
        let error = if address == ACTIVE {
            HostError::Resident(RegistryError::CodeInvalidated)
        } else {
            HostError::Call(CallError::InvalidToken)
        };
        reject(&mut f, error, |f| {
            f.engine
                .complete_active_resident_callback_windows_call(KEY, f.active, 0, inner)
        });
        let mut expected = f.engine.arena().to_vec();
        expected[..96].copy_from_slice(&f.outer_bytes);
        f.engine.abort_callback(KEY, 2).unwrap();
        assert_eq!(f.engine.arena(), expected);
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
        if address == OUTER {
            reject(
                &mut f,
                HostError::Resident(RegistryError::CodeInvalidated),
                |f| f.engine.complete_resident_call(KEY, f.units[0], 1, 10),
            );
        } else {
            f.engine
                .complete_resident_call(KEY, f.units[0], 1, 10)
                .unwrap();
            assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap().eip, 0x3005);
        }
    }
}

#[test]
fn captured_arguments_and_return_frames_ignore_public_ram_and_receipt_tampering() {
    for foreign in [false, true] {
        let mut f = fixture(12, foreign, true);
        let (inner, state) = capture(
            &mut f,
            WindowsApi32::SetLastError,
            CallingConvention32::Stdcall,
            &[LAST_ERROR],
        );
        f.engine.write32(state.registers[4], 0xdead_beef).unwrap();
        f.engine
            .write32(state.registers[4] + 4, 0x1111_2222)
            .unwrap();
        f.engine.write32(0x8ff8, 0xfeed_face).unwrap();
        f.engine.write32(0x8ffc, 0xcafe_babe).unwrap();
        f.engine.arena_mut().unwrap()[100..].fill(0xa5);
        completed(&mut f, inner, state, SENTINEL);
        invoke(&mut f, WindowsApi32::GetLastError, &[], LAST_ERROR);
        finish(&mut f, 47);
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, inner)
        });
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 9);
    }
}

#[test]
fn invalid_provider_shapes_and_callback_exit_process_refuse_before_effects() {
    for (api, convention, arguments) in [
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![1, 1, 0x3000, 4],
        ),
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![0, 0, 0x3000, 4],
        ),
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![0, 65537, 0x3000, 4],
        ),
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![0, 1, 0x1000, 4],
        ),
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![0, 1, 0x3000, 0x20],
        ),
        (
            WindowsApi32::VirtualAlloc,
            CallingConvention32::Stdcall,
            vec![0, 1, 0x3000],
        ),
        (
            WindowsApi32::GetModuleHandleA,
            CallingConvention32::Stdcall,
            vec![1],
        ),
        (
            WindowsApi32::GetLastError,
            CallingConvention32::Stdcall,
            vec![1],
        ),
        (
            WindowsApi32::GetLastError,
            CallingConvention32::Cdecl,
            vec![],
        ),
        (
            WindowsApi32::SetLastError,
            CallingConvention32::Stdcall,
            vec![],
        ),
        (
            WindowsApi32::ExitProcess,
            CallingConvention32::Stdcall,
            vec![],
        ),
        (
            WindowsApi32::ExitProcess,
            CallingConvention32::Cdecl,
            vec![],
        ),
    ] {
        let mut f = fixture(12, true, true);
        invoke(&mut f, WindowsApi32::SetLastError, &[LAST_ERROR], SENTINEL);
        let (inner, _) = capture(&mut f, api, convention, &arguments);
        reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
            complete(f, inner)
        });
        if api == WindowsApi32::ExitProcess {
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                f.engine
                    .complete_active_resident_callback_call(KEY, f.active, 2, inner, 44)
            });
        }
        f.engine.abandon_call(KEY, inner).unwrap();
        invoke(&mut f, WindowsApi32::GetLastError, &[], LAST_ERROR);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 9);
    }
    let mut f = fixture(12, false, false);
    reject(&mut f, HostError::Call(CallError::Busy), |f| {
        f.engine
            .complete_active_resident_callback_windows_call(KEY, f.units[1], 2, 0)
    });
    let mut f = base_fixture(12);
    let outer = f
        .engine
        .capture_call(KEY, f.generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = f
        .engine
        .begin_callback(KEY, f.generation, outer.token, HOME, HOME + 0x200, 18, &[])
        .unwrap();
    reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
        f.engine
            .complete_active_resident_callback_windows_call(KEY, f.units[1], callback.token, 0)
    });
}

#[test]
fn committed_allocation_and_last_error_survive_neutral_abort_and_private_outer_restore() {
    for foreign in [false, true] {
        let mut f = fixture(12, foreign, true);
        invoke(&mut f, WindowsApi32::SetLastError, &[LAST_ERROR], SENTINEL);
        invoke(
            &mut f,
            WindowsApi32::VirtualAlloc,
            &[0, 4097, 0x3000, 4],
            WINDOW,
        );
        f.engine.write32(WINDOW, 0x4433_2211).unwrap();
        f.engine.write32(WINDOW + 8192 - 4, 0x8877_6655).unwrap();
        let (dead_inner, _) = capture(
            &mut f,
            WindowsApi32::ExitProcess,
            CallingConvention32::Stdcall,
            &[],
        );
        reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
            complete(f, dead_inner)
        });
        let before = saved(&f);
        let mut expected = before.arena.clone();
        expected[..96].copy_from_slice(&f.outer_bytes);
        f.engine.abort_callback(KEY, f.callback_token).unwrap();
        assert_eq!(f.engine.arena(), expected);
        assert_eq!(saved(&f).ram, before.ram);
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 11);
        reject(&mut f, HostError::Call(CallError::InvalidToken), |f| {
            complete(f, dead_inner)
        });
        f.engine
            .complete_resident_call(KEY, f.units[0], 1, 44)
            .unwrap();
        f.engine.write32(0x8ff4, HOME + 1).unwrap();
        let mut state = f.outer_state;
        state.eip = HOME + 0x100;
        state.registers[4] = 0x8ff4;
        stop(
            &mut f.engine,
            state,
            ExitReason::Gate {
                id: WindowsApi32::GetLastError.id(),
            },
        );
        let call = f
            .engine
            .capture_resident_call(KEY, f.units[1], CallingConvention32::Stdcall, 0)
            .unwrap();
        assert_eq!(call.token, dead_inner + 1);
        f.engine
            .complete_resident_windows_call(KEY, f.units[1], call.token)
            .unwrap();
        assert_eq!(
            decode_state(&f.engine.arena()[..56]).unwrap().registers[0],
            LAST_ERROR
        );
        assert_eq!(
            &ram(&f.engine, WINDOW).unwrap()[..4],
            &0x4433_2211_u32.to_le_bytes()
        );
        assert_eq!(
            &ram(&f.engine, WINDOW + 4096).unwrap()[4092..],
            &0x8877_6655_u32.to_le_bytes()
        );
    }
}

#[test]
fn active_callback_exit_process_is_a_terminal_provider_outcome() {
    let mut f = fixture(12, false, true);
    let (inner, _) = capture(
        &mut f,
        WindowsApi32::ExitProcess,
        CallingConvention32::Stdcall,
        &[0],
    );
    assert_eq!(complete(&mut f, inner), Ok(()));
}

fn terminal_record(code: u32) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(b"R3EX");
    bytes[4..8].copy_from_slice(&0x0001_0004_u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&40_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&9_u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&code.to_le_bytes());
    bytes
}

fn publish_terminal(f: &mut Fixture, inner: u32, state: State32, code: u32) {
    let before = saved(f);
    let mut expected = before.arena.clone();
    expected[56..96].copy_from_slice(&terminal_record(code));
    assert_eq!(complete(f, inner), Ok(()));
    assert_eq!(f.engine.arena(), expected);
    assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap(), state);
    assert_eq!(f.engine.arena_address(), before.pointer);
    assert_eq!(f.engine.generation(), before.generation);
    assert_eq!(f.engine.key(), KEY);
    assert!(f.engine.is_open());
}

fn terminal_reject<T: std::fmt::Debug + PartialEq>(
    f: &mut Fixture,
    error: HostError,
    operation: impl FnOnce(&mut Fixture) -> Result<T, HostError>,
) {
    let before = saved(f);
    assert_eq!(operation(f), Err(error));
    assert_eq!(saved(f), before);
}

fn diagnostic_page(engine: &mut EngineInstance, address: u32) -> Vec<u8> {
    let before = engine.arena().to_vec();
    let mut bytes = Vec::with_capacity(4096);
    for offset in (0..4096).step_by(4) {
        engine.read32(address + offset).unwrap();
        assert_eq!(&engine.arena()[116..120], &0_u32.to_le_bytes());
        bytes.extend_from_slice(&engine.arena()[120..124]);
    }
    assert_eq!(&engine.arena()[..100], &before[..100]);
    assert_eq!(&engine.arena()[140..], &before[140..]);
    bytes
}

#[test]
fn callback_exit_uses_saved_argument_and_preserves_cpu_pages_and_committed_effects() {
    for foreign in [false, true] {
        for code in [0, u32::MAX] {
            let mut f = fixture(12, foreign, true);
            invoke(&mut f, WindowsApi32::SetLastError, &[LAST_ERROR], SENTINEL);
            invoke(
                &mut f,
                WindowsApi32::VirtualAlloc,
                &[0, 4097, 0x3000, 4],
                WINDOW,
            );
            f.engine.write32(WINDOW, 0x4433_2211).unwrap();
            f.engine.write32(WINDOW + 8192 - 4, 0x8877_6655).unwrap();
            let (inner, state) = capture(
                &mut f,
                WindowsApi32::ExitProcess,
                CallingConvention32::Stdcall,
                &[code],
            );
            terminal_reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                f.engine.complete_active_resident_callback_call(
                    KEY,
                    f.active,
                    f.callback_token,
                    inner,
                    123,
                )
            });
            f.engine.write32(state.registers[4], 0xdead_beef).unwrap();
            f.engine
                .write32(state.registers[4] + 4, code ^ u32::MAX)
                .unwrap();
            f.engine.write32(0x8ff8, 0xdead_0018).unwrap();
            f.engine.write32(0x8ffc, 0xdead_0017).unwrap();
            f.engine.arena_mut().unwrap()[100..].fill(0xa5);
            let addresses = ORIGINAL_PAGES
                .into_iter()
                .chain([WINDOW, WINDOW + 4096])
                .collect::<Vec<_>>();
            let pages = addresses
                .iter()
                .map(|address| ram(&f.engine, *address).unwrap())
                .collect::<Vec<_>>();
            publish_terminal(&mut f, inner, state, code);
            for (address, expected) in addresses.into_iter().zip(pages) {
                assert_eq!(diagnostic_page(&mut f.engine, address), expected);
            }
            terminal_reject(&mut f, HostError::ProcessExited, |f| complete(f, inner));
        }
    }
}

#[test]
fn callback_exit_authority_state_and_cancel_priorities_leave_pending_retryable() {
    for foreign in [false, true] {
        let mut f = fixture(12, foreign, true);
        let (inner, state) = capture(
            &mut f,
            WindowsApi32::ExitProcess,
            CallingConvention32::Stdcall,
            &[u32::MAX],
        );
        for (key, unit, callback, token, error) in [
            (KEY ^ 1, f.active, 0, 0, HostError::InvalidArtifact),
            (
                KEY,
                f.active ^ (1_u64 << 32),
                0,
                0,
                HostError::Resident(RegistryError::InvalidUnit),
            ),
            (
                KEY,
                f.units[3],
                f.callback_token,
                inner,
                HostError::Call(CallError::Busy),
            ),
            (
                KEY,
                f.active,
                0,
                inner,
                HostError::Call(CallError::InvalidToken),
            ),
            (
                KEY,
                f.active,
                f.callback_token,
                0,
                HostError::Call(CallError::InvalidToken),
            ),
            (
                KEY,
                f.active,
                f.callback_token,
                1,
                HostError::Call(CallError::Busy),
            ),
        ] {
            reject(&mut f, error, |f| {
                f.engine
                    .complete_active_resident_callback_windows_call(key, unit, callback, token)
            });
        }
        reject(&mut f, HostError::Call(CallError::Busy), |f| {
            f.engine.complete_resident_windows_call(KEY, f.units[0], 1)
        });
        let frozen = f.engine.arena()[..96].to_vec();
        for offset in [0, 52, 56, 76] {
            f.engine.arena_mut().unwrap()[offset] ^= 1;
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
                .copy_from_slice(&1_u32.to_le_bytes());
            reject(&mut f, HostError::Call(CallError::StateChanged), |f| {
                complete(f, inner)
            });
            f.engine.arena_mut().unwrap()[..96].copy_from_slice(&frozen);
            reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
                complete(f, inner)
            });
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
        }
        publish_terminal(&mut f, inner, state, u32::MAX);
    }
}

#[test]
fn callback_exit_shapes_refuse_after_cancel_without_consuming_callback_or_inner() {
    for foreign in [false, true] {
        for (convention, arguments) in [
            (CallingConvention32::Stdcall, vec![]),
            (CallingConvention32::Stdcall, vec![0, 1]),
            (CallingConvention32::Cdecl, vec![0]),
            (CallingConvention32::Thiscall, vec![0]),
        ] {
            let mut f = fixture(12, foreign, true);
            let (inner, _) = capture(&mut f, WindowsApi32::ExitProcess, convention, &arguments);
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
                .copy_from_slice(&1_u32.to_le_bytes());
            reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
                complete(f, inner)
            });
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                complete(f, inner)
            });
            reject(&mut f, HostError::Call(CallError::InvalidRequest), |f| {
                f.engine.complete_active_resident_callback_call(
                    KEY,
                    f.active,
                    f.callback_token,
                    inner,
                    99,
                )
            });
            f.engine.abandon_call(KEY, inner).unwrap();
            let (retry, state) = capture(
                &mut f,
                WindowsApi32::ExitProcess,
                CallingConvention32::Stdcall,
                &[0],
            );
            assert_eq!(retry, inner + 1);
            publish_terminal(&mut f, retry, state, 0);
        }
    }
}

#[test]
fn callback_exit_stale_active_home_and_outer_precede_bad_inner_and_allow_abort() {
    for foreign in [false, true] {
        for page in [OUTER, HOME, ACTIVE] {
            let mut f = fixture(12, foreign, true);
            let (inner, _) = capture(
                &mut f,
                WindowsApi32::ExitProcess,
                CallingConvention32::Stdcall,
                &[99],
            );
            f.engine.write8(page, 0x90).unwrap();
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4]
                .copy_from_slice(&1_u32.to_le_bytes());
            let relevant = page != ACTIVE || foreign;
            let error = if relevant {
                HostError::Resident(RegistryError::CodeInvalidated)
            } else {
                HostError::Call(CallError::InvalidToken)
            };
            reject(&mut f, error, |f| {
                f.engine.complete_active_resident_callback_windows_call(
                    KEY,
                    f.active,
                    f.callback_token,
                    0,
                )
            });
            if relevant {
                reject(
                    &mut f,
                    HostError::Resident(RegistryError::CodeInvalidated),
                    |f| complete(f, inner),
                );
            } else {
                reject(&mut f, HostError::Call(CallError::Cancelled), |f| {
                    complete(f, inner)
                });
            }
            let mut expected = f.engine.arena().to_vec();
            expected[..96].copy_from_slice(&f.outer_bytes);
            f.engine.abort_callback(KEY, f.callback_token).unwrap();
            assert_eq!(f.engine.arena(), expected);
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET..CANCEL_OFFSET + 4].fill(0);
            if page == OUTER {
                reject(
                    &mut f,
                    HostError::Resident(RegistryError::CodeInvalidated),
                    |f| f.engine.complete_resident_call(KEY, f.units[0], 1, 99),
                );
            } else {
                f.engine
                    .complete_resident_call(KEY, f.units[0], 1, 99)
                    .unwrap();
            }
        }
    }
}

#[test]
fn callback_terminal_latch_precedes_live_controls_and_close_keeps_tombstone() {
    for foreign in [false, true] {
        let mut f = fixture(12, foreign, true);
        let (inner, state) = capture(
            &mut f,
            WindowsApi32::ExitProcess,
            CallingConvention32::Stdcall,
            &[0],
        );
        publish_terminal(&mut f, inner, state, 0);
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine
                .complete_active_resident_callback_windows_call(KEY ^ 1, 0, 0, 0)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine
                .complete_active_resident_callback_call(KEY ^ 1, 0, 0, 0, 0)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.complete_resident_windows_call(KEY, f.units[0], 1)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.complete_resident_call(KEY, f.units[0], 1, 0)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine
                .finish_resident_callback(KEY, f.units[1], f.callback_token)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.abort_callback(KEY, f.callback_token)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.guard_resident(0, 0)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.guard_dispatch_entry(0)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| f.engine.guard(0, 0));
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.map(u32::MAX, 0, u32::MAX)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.write8(u32::MAX, u32::MAX)
        });
        terminal_reject(&mut f, HostError::ProcessExited, |f| {
            f.engine.compile_resident(0)
        });
        assert!(matches!(
            f.engine.arena_mut(),
            Err(HostError::ProcessExited)
        ));
        assert!(matches!(
            f.engine.dispatcher_bytes(0),
            Err(HostError::ProcessExited)
        ));
        let before_read = f.engine.arena().to_vec();
        f.engine.read8(STACK).unwrap();
        f.engine.read16(STACK).unwrap();
        f.engine.read32(STACK).unwrap();
        assert_eq!(&f.engine.arena()[..100], &before_read[..100]);
        assert_eq!(&f.engine.arena()[140..], &before_read[140..]);
        let tombstone = f.engine.arena().to_vec();
        let pointer = f.engine.arena_address();
        f.engine.close();
        f.engine.close();
        assert!(!f.engine.is_open());
        assert_eq!(f.engine.arena(), tombstone);
        assert_eq!(f.engine.arena_address(), pointer);
        assert_eq!(f.engine.key(), KEY);
        assert_eq!(f.engine.generation(), 0);
        terminal_reject(&mut f, HostError::Closed, |f| {
            f.engine
                .complete_active_resident_callback_windows_call(KEY ^ 1, 0, 0, 0)
        });
        terminal_reject(&mut f, HostError::Closed, |f| {
            f.engine.abort_callback(KEY ^ 1, 0)
        });
        terminal_reject(&mut f, HostError::Closed, |f| f.engine.read8(STACK));
    }
}
