use super::*;
use crate::{
    abi::{
        arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
        x86::{decode_state, encode_exit_v3, encode_exit_v4, encode_state},
    },
    cpu::{
        ExecutionExit,
        dbt::{RegistryUsage, UnitId},
        x86::State32,
    },
    loader::ImageMetadata32,
    memory::GuestAddress,
    process::ResidentInstallation,
    windows::{CallFrame32, CallingConvention32, WindowsOutcome32},
};

const KEY: u64 = 0xe344_abcd_1234_5678;
const BASE: u32 = 0x0040_0000;
const ENTRY: u32 = BASE + 4096;
const STALE: u32 = 0x6000_0000;
const RAM: u32 = 0x7000_0000;
const STACK: u32 = 0x8000;
const ESP: u32 = 0x8ff8;
const EXIT: u32 = 0x0001_0003;
const LAST_ERROR: u32 = 0x89ab_cdef;

struct Fixture {
    engine: EngineInstance,
    current: UnitId,
    stale: UnitId,
}

fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}

fn descriptors(engine: &mut EngineInstance, entry: u32, gate: bool) {
    let words = if gate {
        vec![entry, 2, entry, EXIT]
    } else {
        vec![entry, 1]
    };
    for (index, word) in words.into_iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
}

fn thread_frame(engine: &EngineInstance, words: u32) -> CallFrame32 {
    let mut state = State32::default();
    state.registers[0] = 0x1357_9bdf;
    state.registers[4] = RAM;
    CallFrame32::capture(
        engine.memory.as_ref().unwrap(),
        state,
        CallingConvention32::Stdcall,
        words,
    )
    .unwrap()
}

fn last_error(engine: &EngineInstance) -> u32 {
    match engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &thread_frame(engine, 0),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .0
    {
        WindowsOutcome32::Return(value) => value,
        WindowsOutcome32::ExitProcess(_) => panic!("getlasterror must return"),
    }
}

fn fixture() -> Fixture {
    // private publication fixture; public pe tests and actual wasm own parsing/execution.
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    engine.map(BASE, 1, 1).unwrap();
    engine.map(ENTRY, 1, 7).unwrap();
    engine.map(STALE, 1, 7).unwrap();
    engine.map(RAM, 1, 3).unwrap();
    write(&mut engine, ENTRY, &[0x0f, 0x0b]);
    write(&mut engine, STALE, &[0x90]);
    write(&mut engine, RAM, &ENTRY.to_le_bytes());
    write(&mut engine, RAM + 4, &LAST_ERROR.to_le_bytes());
    let (_, thread) = engine
        .windows_thread
        .prepare(
            WindowsApi32::SetLastError,
            &thread_frame(&engine, 1),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap();
    engine.windows_thread = thread;
    engine.image = Some(ImageMetadata32 {
        image_base: BASE,
        image_size: 0x4000,
        entry_point: ENTRY,
        mapped_pages: 2,
    });
    descriptors(&mut engine, ENTRY, true);
    engine.compile_with_gates(1, 1).unwrap();
    let current = engine.compile_resident_with_gates(1, 1).unwrap();
    engine
        .acknowledge_resident_installation(KEY, current.get(), 0)
        .unwrap();
    descriptors(&mut engine, STALE, false);
    let stale = engine.compile_resident(1).unwrap();
    engine
        .acknowledge_resident_installation(KEY, stale.get(), 1)
        .unwrap();
    write(&mut engine, STALE, &[0x90]);
    engine.start_loaded_image(STACK, 1).unwrap();
    assert_eq!(last_error(&engine), LAST_ERROR);
    Fixture {
        engine,
        current,
        stale,
    }
}

fn capture(f: &mut Fixture, resident: bool, code: u32) -> u32 {
    write(&mut f.engine, ESP, &(ENTRY + 15).to_le_bytes());
    write(&mut f.engine, ESP + 4, &code.to_le_bytes());
    let mut state = State32::default();
    state.registers[0] = 0x1357_9bdf;
    state.registers[4] = ESP;
    state.eip = ENTRY;
    let arena = f.engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: EXIT },
        },
        &mut arena[56..96],
    )
    .unwrap();
    match resident {
        false => f
            .engine
            .capture_call(KEY, f.engine.generation, CallingConvention32::Stdcall, 1),
        true => {
            f.engine
                .capture_resident_call(KEY, f.current.get(), CallingConvention32::Stdcall, 1)
        }
    }
    .unwrap()
    .token
}

fn complete(f: &mut Fixture, resident: bool, token: u32) -> Result<(), HostError> {
    if resident {
        f.engine
            .complete_resident_windows_call(KEY, f.current.get(), token)
    } else {
        f.engine
            .complete_windows_call(KEY, f.engine.generation, token)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Owners {
    memory_identity: u64,
    mapped_pages: u32,
    ram: Vec<Vec<u8>>,
    image: Option<ImageMetadata32>,
    image_started: bool,
    key: u64,
    generation: u32,
    call_token: u32,
    arena_address: usize,
    artifact: Vec<u8>,
    artifact_pointer: usize,
    dispatcher: Vec<u8>,
    dispatcher_pointer: usize,
    registry: String,
    registry_pointer: usize,
    usage: RegistryUsage,
    installations: Vec<Option<ResidentInstallation>>,
    current: Vec<u8>,
    current_pointer: usize,
    stale: Result<(), crate::cpu::dbt::RegistryError>,
    last_error: u32,
}

fn owners(f: &Fixture) -> Owners {
    let e = &f.engine;
    let memory = e.memory.as_ref().unwrap();
    let artifact = e.artifact.as_ref().unwrap().wasm_bytes(memory).unwrap();
    let registry = e.resident.as_ref().unwrap();
    let current = registry
        .get(memory, f.current)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    let dispatcher = e.dispatcher.as_ref().unwrap();
    Owners {
        memory_identity: memory.identity(),
        mapped_pages: memory.mapped_pages(),
        ram: [BASE, ENTRY, STALE, RAM, STACK]
            .into_iter()
            .map(|address| {
                let mut bytes = vec![0; 4096];
                memory.read(GuestAddress(address), &mut bytes).unwrap();
                bytes
            })
            .collect(),
        image: e.image,
        image_started: e.image_started,
        key: e.key,
        generation: e.generation,
        call_token: e.call_token,
        arena_address: e.arena_address(),
        artifact: artifact.to_vec(),
        artifact_pointer: artifact.as_ptr() as usize,
        dispatcher: dispatcher.to_vec(),
        dispatcher_pointer: dispatcher.as_ptr() as usize,
        registry: format!("{registry:?}"),
        registry_pointer: registry as *const _ as usize,
        usage: registry.usage(),
        installations: e.resident_installations.to_vec(),
        current: current.to_vec(),
        current_pointer: current.as_ptr() as usize,
        stale: registry.get(memory, f.stale).map(|_| ()),
        last_error: last_error(e),
    }
}

#[test]
fn terminal_publication_preserves_full_private_owners_memory_versions_and_last_error() {
    for resident in [false, true] {
        let mut f = fixture();
        let token = capture(&mut f, resident, 0xf123_4567);
        let memory = f.engine.memory.as_ref().unwrap();
        let current_snapshot = memory.snapshot_code(GuestAddress(ENTRY), 2).unwrap();
        let stale_snapshot = memory.snapshot_code(GuestAddress(STALE), 1).unwrap();
        write(&mut f.engine, STALE, &[0x90]);
        let before = owners(&f);
        assert_eq!(before.usage.units, 2);
        assert_eq!(
            before.stale,
            Err(crate::cpu::dbt::RegistryError::CodeInvalidated)
        );
        assert!(before.image_started);
        let arena = f.engine.arena().to_vec();
        let owner = f.engine.pending_call.as_ref().unwrap().owner;
        assert_eq!(
            owner,
            if resident {
                PendingOwner::Resident(f.current.get())
            } else {
                PendingOwner::Replacement(f.engine.generation)
            }
        );
        assert_eq!(f.engine.exit_code, None);
        assert_eq!(complete(&mut f, resident, token), Ok(()));
        assert_eq!(owners(&f), before);
        assert_eq!(f.engine.exit_code, Some(0xf123_4567));
        assert!(f.engine.pending_call.is_none());
        assert!(f.engine.callback.is_none());
        assert_eq!(&f.engine.arena()[..56], &arena[..56]);
        assert_eq!(&f.engine.arena()[96..], &arena[96..]);
        let memory = f.engine.memory.as_ref().unwrap();
        assert!(memory.is_code_current(&current_snapshot));
        assert!(!memory.is_code_current(&stale_snapshot));
        assert_eq!(f.engine.memory().map(|_| ()), Err(HostError::ProcessExited));
    }
}

#[test]
fn terminal_completion_needs_no_memory_version_even_when_versions_are_exhausted() {
    let mut f = fixture();
    let token = capture(&mut f, false, u32::MAX);
    f.engine
        .memory
        .as_mut()
        .unwrap()
        .exhaust_versions_for_test();
    let before = owners(&f);
    let snapshot = f
        .engine
        .memory
        .as_ref()
        .unwrap()
        .snapshot_code(GuestAddress(ENTRY), 2)
        .unwrap();
    assert_eq!(complete(&mut f, false, token), Ok(()));
    assert_eq!(owners(&f), before);
    assert_eq!(f.engine.exit_code, Some(u32::MAX));
    assert!(f.engine.memory.as_ref().unwrap().is_code_current(&snapshot));
    let arena = f.engine.arena().to_vec();
    assert_eq!(f.engine.write32(RAM, 0), Err(HostError::ProcessExited));
    assert_eq!(f.engine.arena(), arena);
}

#[test]
fn failed_cancel_state_and_scalar_checks_keep_latch_pending_frame_and_thread_unchanged() {
    let mut f = fixture();
    let token = capture(&mut f, false, 0xf123_4567);
    let frame = f
        .engine
        .pending_call
        .as_ref()
        .unwrap()
        .frame
        .arguments()
        .to_vec();
    let before = owners(&f);
    for mode in [0, 1, 2] {
        if mode == 0 {
            f.engine.arena_mut().unwrap()[16] ^= 1;
        }
        if mode <= 1 {
            f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
        }
        let arena = f.engine.arena().to_vec();
        let expected = match mode {
            0 => CallError::StateChanged,
            1 => CallError::Cancelled,
            _ => CallError::InvalidRequest,
        };
        let result = if mode == 2 {
            f.engine.complete_call(KEY, f.engine.generation, token, 0)
        } else {
            complete(&mut f, false, token)
        };
        assert_eq!(result, Err(HostError::Call(expected)));
        assert_eq!(f.engine.arena(), arena);
        assert_eq!(owners(&f), before);
        assert_eq!(f.engine.exit_code, None);
        assert_eq!(f.engine.pending_call.as_ref().unwrap().token, token);
        assert_eq!(
            f.engine.pending_call.as_ref().unwrap().frame.arguments(),
            frame
        );
        if mode == 0 {
            f.engine.arena_mut().unwrap()[16] ^= 1;
        }
        f.engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
    }
    assert_eq!(complete(&mut f, false, token), Ok(()));
    assert_eq!(f.engine.exit_code, Some(0xf123_4567));
}

#[test]
fn private_exit_latch_survives_raw_cpu_exit_cancel_restore_and_close_releases_owners() {
    let mut f = fixture();
    let token = capture(&mut f, true, 0);
    complete(&mut f, true, token).unwrap();
    let before = owners(&f);
    let mut state = State32::default();
    state.eip = ENTRY;
    state.registers[4] = ESP;
    let arena = f.engine.arena.as_mut().get_mut();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode,
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[CANCEL_OFFSET] = 1;
    let restored = f.engine.arena().to_vec();
    assert_eq!(f.engine.exit_code, Some(0));
    assert_eq!(f.engine.guard(0, 0), Err(HostError::ProcessExited));
    assert_eq!(
        f.engine.guard_resident(0, u64::MAX),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        f.engine.guard_dispatch_entry(0),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        f.engine.complete_windows_call(0, 0, 0),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        f.engine.start_loaded_image(1, u32::MAX),
        Err(HostError::ProcessExited)
    );
    assert_eq!(f.engine.arena(), restored);
    assert_eq!(owners(&f), before);
    f.engine.close();
    assert_eq!(f.engine.exit_code, Some(0));
    assert_eq!(f.engine.memory().map(|_| ()), Err(HostError::Closed));
    assert_eq!(f.engine.read32(RAM), Err(HostError::Closed));
    assert_eq!(f.engine.arena(), restored);
    assert!(f.engine.memory.is_none());
    assert!(f.engine.artifact.is_none());
    assert!(f.engine.resident.is_none());
    assert!(f.engine.dispatcher.is_none());
    assert!(f.engine.pending_call.is_none());
    assert!(f.engine.callback.is_none());
    assert!(f.engine.resident_installations.iter().all(Option::is_none));
}

#[test]
fn public_terminal_codec_cannot_set_private_exit_and_pure_outcome_preserves_thread_state() {
    let mut f = fixture();
    let mut terminal = [0; 40];
    encode_exit_v4(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::ProcessExited { code: u32::MAX },
        },
        &mut terminal,
    )
    .unwrap();
    f.engine.arena_mut().unwrap()[56..96].copy_from_slice(&terminal);
    assert_eq!(f.engine.exit_code, None);
    assert!(f.engine.memory().is_ok());
    assert_eq!(last_error(&f.engine), LAST_ERROR);
    let frame = thread_frame(&f.engine, 1);
    let (outcome, thread) = f
        .engine
        .windows_thread
        .prepare(
            WindowsApi32::ExitProcess,
            &frame,
            crate::windows::ProcessContext32::default(),
        )
        .unwrap();
    assert_eq!(outcome, WindowsOutcome32::ExitProcess(LAST_ERROR));
    assert_eq!(
        thread
            .prepare(
                WindowsApi32::GetLastError,
                &thread_frame(&f.engine, 0),
                crate::windows::ProcessContext32::default()
            )
            .unwrap()
            .0,
        WindowsOutcome32::Return(LAST_ERROR)
    );
    assert_eq!(f.engine.exit_code, None);
    assert_eq!(decode_state(&f.engine.arena()[..56]).unwrap().eip, ENTRY);
    let before = owners(&f);
    f.engine.read32(RAM + 4).unwrap();
    assert_eq!(owners(&f), before);
    assert_eq!(f.engine.exit_code, None);
    assert_eq!(&f.engine.arena()[56..96], &terminal);
}
