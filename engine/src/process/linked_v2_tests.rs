use super::*;
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{decode_state, encode_exit_v3, encode_state},
    },
    cpu::{
        ExecutionExit, ExitReason,
        dbt::{GateSpec, RegistryError, UnitId},
        x86::State32,
    },
    loader::LoadError,
    memory::{AddressSpace, GuestAddress, MemoryError, PageRange, Permissions},
    windows::{CallFrame32, CallingConvention32, WindowsApi32, WindowsOutcome32},
};

const KEY: u64 = 0xe345_abcd_1234_5678;
const BASE: u32 = 0x0040_0000;
const GATE: u32 = 0x7000_0000;
const LAST_ERROR: u32 = 0x89ab_cdef;
const EXIT_CODE: u32 = 0xf123_4567;
const ESP: u32 = 0x8ff8;

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    // independent literal two-section fixture; this is a private lifecycle proof.
    let mut bytes = vec![0; 0x600];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14c),
        (0x86, 2),
        (0x94, 224),
        (0x96, 0x0103),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        put16(&mut bytes, at, value);
    }
    for (at, value) in [
        (0xa8, 0x1000),
        (0xb4, BASE),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x5000),
        (0xd4, 512),
        (0xf4, 16),
        (0x100, 0x3000),
        (0x104, 40),
        (0x158, 0x3080),
        (0x15c, 16),
    ] {
        put32(&mut bytes, at, value);
    }
    for (at, rva, size, raw, flags) in [
        (0x178, 0x1000, 16, 0x200, 0x6000_0020),
        (0x1a0, 0x3000, 512, 0x400, 0x4000_0040),
    ] {
        bytes[at..at + 8].copy_from_slice(b"authored");
        for (offset, value) in [(8, size), (12, rva), (16, 512), (20, raw), (36, flags)] {
            put32(&mut bytes, at + offset, value);
        }
    }
    bytes[0x200..0x205].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    for (at, value) in [(0x400, 0x3040), (0x40c, 0x30c0), (0x410, 0x3080)] {
        put32(&mut bytes, at, value);
    }
    for (index, (rva, symbol)) in [
        (0x3100, b"GetLastError\0".as_slice()),
        (0x3120, b"SetLastError\0"),
        (0x3140, b"ExitProcess\0"),
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut bytes, 0x440 + index * 4, rva);
        put32(&mut bytes, 0x480 + index * 4, rva);
        let at = 0x400 + (rva - 0x3000) as usize + 2;
        bytes[at..at + symbol.len()].copy_from_slice(symbol);
    }
    bytes[0x4c0..0x4cd].copy_from_slice(b"kernel32.dll\0");
    bytes
}

fn probe_frame(words: u32, argument: u32) -> CallFrame32 {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &BASE.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x1004), &argument.to_le_bytes())
        .unwrap();
    let mut state = State32::default();
    state.registers[4] = 0x1000;
    CallFrame32::capture(&memory, state, CallingConvention32::Stdcall, words).unwrap()
}

fn last_error(engine: &EngineInstance) -> u32 {
    match engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &probe_frame(0, 0),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .0
    {
        WindowsOutcome32::Return(value) => value,
        WindowsOutcome32::ExitProcess(_) => panic!("getlasterror must return"),
        WindowsOutcome32::Allocate { .. } | WindowsOutcome32::Release { .. } => {
            panic!("getlasterror must return")
        }
    }
}

fn seed_last_error(engine: &mut EngineInstance) {
    engine.windows_thread = engine
        .windows_thread
        .prepare(
            WindowsApi32::SetLastError,
            &probe_frame(1, LAST_ERROR),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .1;
}

fn descriptors(engine: &mut EngineInstance, gates: &[GateSpec]) {
    let words: Vec<u32> = gates
        .iter()
        .flat_map(|gate| [gate.entry.0, 2])
        .chain(gates.iter().flat_map(|gate| [gate.entry.0, gate.id]))
        .collect();
    for (index, word) in words.into_iter().enumerate() {
        put32(
            engine.arena_mut().unwrap(),
            TRANSFER_OFFSET + index * 4,
            word,
        );
    }
}

struct Fixture {
    engine: EngineInstance,
    gates: [GateSpec; 3],
    current: UnitId,
    stale: UnitId,
}

fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    let linked = engine.load_pe32_linked_v2_at(&image(), BASE, GATE).unwrap();
    descriptors(&mut engine, &linked.gates);
    engine.compile_with_gates(3, 3).unwrap();
    let current = engine.compile_resident_with_gates(3, 3).unwrap();
    engine
        .acknowledge_resident_installation(KEY, current.get(), 0)
        .unwrap();
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET, BASE + 0x1000);
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET + 4, 5);
    let stale = engine.compile_resident(1).unwrap();
    engine
        .acknowledge_resident_installation(KEY, stale.get(), 1)
        .unwrap();
    engine.protect(BASE + 0x1000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(BASE + 0x1000), &[0xb8, 42, 0, 0, 0])
        .unwrap();
    engine.protect(BASE + 0x1000, 1, 5).unwrap();
    engine.start_loaded_image(0x8000, 1).unwrap();
    seed_last_error(&mut engine);
    Fixture {
        engine,
        gates: linked.gates,
        current,
        stale,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Owners {
    identity: u64,
    pages: u32,
    image: Option<crate::loader::ImageMetadata32>,
    started: bool,
    key: u64,
    generation: u32,
    arena_pointer: usize,
    dispatcher: Vec<u8>,
    dispatcher_pointer: usize,
    artifact: Vec<u8>,
    artifact_pointer: usize,
    registry: String,
    registry_pointer: usize,
    installations: Vec<Option<crate::process::ResidentInstallation>>,
    current: Vec<u8>,
    current_pointer: usize,
    stale: Result<(), RegistryError>,
    ram: Vec<Vec<u8>>,
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
        identity: memory.identity(),
        pages: memory.mapped_pages(),
        image: e.image,
        started: e.image_started,
        key: e.key,
        generation: e.generation,
        arena_pointer: e.arena_address(),
        dispatcher: dispatcher.clone(),
        dispatcher_pointer: dispatcher.as_ptr() as usize,
        artifact: artifact.to_vec(),
        artifact_pointer: artifact.as_ptr() as usize,
        registry: format!("{registry:?}"),
        registry_pointer: registry as *const _ as usize,
        installations: e.resident_installations.to_vec(),
        current: current.to_vec(),
        current_pointer: current.as_ptr() as usize,
        stale: registry.get(memory, f.stale).map(|_| ()),
        ram: [BASE, BASE + 0x1000, BASE + 0x3000, GATE, 0x8000]
            .into_iter()
            .map(|address| {
                let mut bytes = vec![0; 4096];
                memory.read(GuestAddress(address), &mut bytes).unwrap();
                bytes
            })
            .collect(),
        last_error: last_error(e),
    }
}

fn capture(f: &mut Fixture, resident: bool) -> u32 {
    // native typed gate model; actual wasm independently proves call/iat execution.
    f.engine.write32(ESP, BASE + 0x1005).unwrap();
    f.engine.write32(ESP + 4, EXIT_CODE).unwrap();
    let mut state = State32::default();
    state.registers[0] = LAST_ERROR;
    state.registers[4] = ESP;
    state.eip = f.gates[2].entry.0;
    let arena = f.engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: f.gates[2].id },
        },
        &mut arena[56..96],
    )
    .unwrap();
    if resident {
        f.engine
            .capture_resident_call(KEY, f.current.get(), CallingConvention32::Stdcall, 1)
    } else {
        f.engine
            .capture_call(KEY, f.engine.generation, CallingConvention32::Stdcall, 1)
    }
    .unwrap()
    .token
}

#[test]
fn failed_pristine_candidate_preserves_identity_versions_and_success_publishes_only_private_image()
{
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    seed_last_error(&mut engine);
    engine.call_token = 41;
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(29).wrapping_add(3);
    }
    let arena = engine.arena().to_vec();
    let identity = engine.memory().unwrap().identity();
    let dispatcher = engine.dispatcher.as_ref().unwrap().clone();
    let dispatcher_pointer = engine.dispatcher.as_ref().unwrap().as_ptr();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    for (bytes, gate, error) in [
        (b"bad".to_vec(), GATE, LoadError::Malformed),
        (image(), BASE + 0x2000, LoadError::Malformed),
    ] {
        assert_eq!(
            engine.load_pe32_linked_v2_at(&bytes, BASE, gate),
            Err(HostError::Loader(error))
        );
        assert_eq!(engine.memory().unwrap().identity(), identity);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
        assert_eq!(engine.arena(), arena);
        assert!(engine.image.is_none());
        assert!(!engine.image_started);
        assert!(engine.exit_code.is_none());
        assert!(engine.pending_call.is_none());
        assert!(engine.callback.is_none());
        assert_eq!(engine.call_token, 41);
        assert_eq!(last_error(&engine), LAST_ERROR);
    }
    assert_eq!(
        engine.map(0x8000, 1, 3),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    let linked = engine.load_pe32_linked_v2_at(&image(), BASE, GATE).unwrap();
    assert_ne!(engine.memory().unwrap().identity(), identity);
    assert_eq!(engine.image, Some(linked.image));
    assert_eq!(linked.image.entry_point, BASE + 0x1000);
    assert_eq!(linked.image.mapped_pages, 4);
    assert!(!engine.image_started);
    assert!(engine.exit_code.is_none());
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.call_token, 41);
    assert_eq!(last_error(&engine), LAST_ERROR);
    assert_eq!(engine.dispatcher.as_ref().unwrap(), &dispatcher);
    assert_eq!(
        engine.dispatcher.as_ref().unwrap().as_ptr(),
        dispatcher_pointer
    );
}

#[test]
fn private_startup_authority_and_image_latch_survive_retained_current_stale_owners_and_unmap() {
    let mut f = fixture();
    assert_eq!(
        decode_state(&f.engine.arena()[..56]).unwrap().eip,
        BASE + 0x1000
    );
    assert_eq!(f.engine.image.unwrap().entry_point, BASE + 0x1000);
    assert!(f.engine.image_started);
    let before = owners(&f);
    assert_eq!(before.stale, Err(RegistryError::CodeInvalidated));
    assert_eq!(
        f.engine.load_pe32_linked_v2_at(b"bad", 0, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(owners(&f), before);
    let arena = f.engine.arena().to_vec();
    let loaded_image = f.engine.image;
    for address in [BASE, BASE + 0x1000, BASE + 0x3000, GATE, 0x8000] {
        f.engine.unmap(address, 1).unwrap();
    }
    let identity = f.engine.memory().unwrap().identity();
    assert_eq!(f.engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(
        f.engine.load_pe32_linked_v2_at(&image(), BASE, GATE),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        f.engine.start_loaded_image(0x8000, 1),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(f.engine.image, loaded_image);
    assert!(f.engine.image_started);
    assert!(f.engine.exit_code.is_none());
    assert_eq!(f.engine.memory().unwrap().identity(), identity);
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(
        f.engine.resident_installations,
        before.installations.as_slice()
    );
    assert_eq!(last_error(&f.engine), LAST_ERROR);
}

#[test]
fn pending_saved_frame_blocks_candidate_publication_without_losing_private_call_or_code_owners() {
    for resident in [false, true] {
        let mut f = fixture();
        let token = capture(&mut f, resident);
        let before = owners(&f);
        let arena = f.engine.arena().to_vec();
        let pending = f.engine.pending_call.as_ref().unwrap();
        let owner = pending.owner;
        let frame = pending.frame.arguments().to_vec();
        let call_token = f.engine.call_token;
        assert_eq!(
            f.engine.load_pe32_linked_v2_at(b"bad", 0, 0),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(owners(&f), before);
        assert_eq!(f.engine.arena(), arena);
        let pending = f.engine.pending_call.as_ref().unwrap();
        assert_eq!(pending.token, token);
        assert_eq!(pending.owner, owner);
        assert_eq!(pending.frame.arguments(), frame);
        assert_eq!(f.engine.call_token, call_token);
        assert!(f.engine.exit_code.is_none());
        assert!(f.engine.callback.is_none());
    }
}

#[test]
fn linked_terminal_keeps_distinct_last_error_memory_versions_and_all_retained_owners_then_close_wins()
 {
    for resident in [false, true] {
        let mut f = fixture();
        let token = capture(&mut f, resident);
        f.engine.write32(ESP + 4, LAST_ERROR).unwrap();
        let before = owners(&f);
        let snapshot = f
            .engine
            .memory
            .as_ref()
            .unwrap()
            .snapshot_code(GuestAddress(GATE + 32), 2)
            .unwrap();
        let arena = f.engine.arena().to_vec();
        let call_token = f.engine.call_token;
        let result = if resident {
            f.engine
                .complete_resident_windows_call(KEY, f.current.get(), token)
        } else {
            f.engine
                .complete_windows_call(KEY, f.engine.generation, token)
        };
        assert_eq!(result, Ok(()));
        assert_eq!(f.engine.exit_code, Some(EXIT_CODE));
        assert_eq!(last_error(&f.engine), LAST_ERROR);
        assert_eq!(owners(&f), before);
        assert_eq!(f.engine.call_token, call_token);
        assert!(f.engine.pending_call.is_none());
        assert!(f.engine.callback.is_none());
        assert_eq!(&f.engine.arena()[..56], &arena[..56]);
        assert_eq!(&f.engine.arena()[96..], &arena[96..]);
        assert!(f.engine.memory.as_ref().unwrap().is_code_current(&snapshot));
        let terminal = f.engine.arena().to_vec();
        assert_eq!(
            f.engine.load_pe32_linked_v2_at(b"bad", 0, 0),
            Err(HostError::ProcessExited)
        );
        assert_eq!(
            f.engine.load_pe32_linked_at(b"bad", 0, 0),
            Err(HostError::ProcessExited)
        );
        assert_eq!(f.engine.arena(), terminal);
        assert_eq!(owners(&f), before);
        f.engine.close();
        assert_eq!(
            f.engine.load_pe32_linked_v2_at(b"bad", 0, 0),
            Err(HostError::Closed)
        );
        assert_eq!(f.engine.arena(), terminal);
        assert_eq!(f.engine.exit_code, Some(EXIT_CODE));
        assert!(f.engine.memory.is_none());
        assert!(f.engine.artifact.is_none());
        assert!(f.engine.resident.is_none());
        assert!(f.engine.resident_installations.iter().all(Option::is_none));
    }
}
