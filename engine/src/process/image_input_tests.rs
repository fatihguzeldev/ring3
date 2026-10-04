use super::*;
use crate::{
    abi::{
        arena::{CANCEL_OFFSET, TRANSFER_OFFSET},
        callback::CallbackRecord32,
    },
    cpu::{dbt::RegistryError, x86::State32},
    loader::{ImageMetadata32, LoadError},
    memory::{AddressSpace, GuestAddress, MemoryError, PageRange, Permissions},
    process::{
        CallError,
        call::{PendingCall, PendingOwner},
        callback::{SuspendedCallback, SuspendedRecord},
    },
    windows::{CallFrame32, CallingConvention32, WindowsApi32, WindowsOutcome32},
};

const KEY: u64 = 0x9349_1234_abcd_5678;
const BASE: u32 = 0x0200_0000;
const GATE: u32 = 0x7200_0000;
const LAST_ERROR: u32 = 0x89ab_cdef;

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn frame(words: u32) -> CallFrame32 {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &0x1234_5678_u32.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x1004), &LAST_ERROR.to_le_bytes())
        .unwrap();
    let mut state = State32::default();
    state.registers[4] = 0x1000;
    CallFrame32::capture(&memory, state, CallingConvention32::Stdcall, words).unwrap()
}

fn seed_last_error(engine: &mut EngineInstance) {
    engine.windows_thread = engine
        .windows_thread
        .prepare(
            WindowsApi32::SetLastError,
            &frame(1),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .1;
}

fn last_error(engine: &EngineInstance) -> u32 {
    match engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &frame(0),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .0
    {
        WindowsOutcome32::Return(value) => value,
        WindowsOutcome32::ExitProcess(_) => panic!("getlasterror must return"),
        WindowsOutcome32::Allocate { .. } => panic!("getlasterror must return"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Input {
    total: u32,
    bytes: Vec<u8>,
    pointer: usize,
    capacity: usize,
}

fn input(engine: &EngineInstance) -> Option<Input> {
    engine.image_input.as_ref().map(|input| Input {
        total: input.total,
        bytes: input.bytes.clone(),
        pointer: input.bytes.as_ptr() as usize,
        capacity: input.bytes.capacity(),
    })
}

fn call(call: &PendingCall) -> String {
    format!(
        "{:?}/{:?}/{:?}/{:?}/{:?}",
        call.token, call.owner, call.frame, call.state, call.exit
    )
}

fn pending() -> PendingCall {
    // typed native ownership model; it does not claim an executed guest call.
    PendingCall {
        token: 41,
        owner: PendingOwner::Replacement(17),
        frame: frame(1),
        state: [0x51; 56],
        exit: [0x62; 40],
    }
}

fn callback() -> SuspendedCallback {
    SuspendedCallback {
        outer: pending(),
        record: SuspendedRecord::Replacement(CallbackRecord32 {
            token: 42,
            outer_token: 41,
            phase: 1,
            outcome: 0,
            entry_pc: 0x2000,
            entry_esp: 0x1000,
            return_pc: 0x3000,
            return_id: 1,
            stack_words: 1,
            result: 0,
            generation: 17,
        }),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Owners {
    identity: Option<u64>,
    pages: Option<u32>,
    arena: Vec<u8>,
    arena_pointer: usize,
    image: Option<ImageMetadata32>,
    started: bool,
    exit: Option<u32>,
    key: u64,
    generation: u32,
    token: u32,
    dispatcher: Option<(usize, Vec<u8>)>,
    pending: Option<String>,
    callback: Option<String>,
    last_error: u32,
}

fn owners(engine: &EngineInstance) -> Owners {
    Owners {
        identity: engine.memory.as_ref().map(AddressSpace::identity),
        pages: engine.memory.as_ref().map(AddressSpace::mapped_pages),
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        image: engine.image,
        started: engine.image_started,
        exit: engine.exit_code,
        key: engine.key,
        generation: engine.generation,
        token: engine.call_token,
        dispatcher: engine
            .dispatcher
            .as_ref()
            .map(|bytes| (bytes.as_ptr() as usize, bytes.clone())),
        pending: engine.pending_call.as_ref().map(call),
        callback: engine.callback.as_ref().map(|callback| {
            let record = match callback.record {
                SuspendedRecord::Replacement(record) => format!("{record:?}"),
                SuspendedRecord::Resident { .. } => {
                    panic!("fixture is a replacement callback model")
                }
            };
            format!("{record}/{}", call(&callback.outer))
        }),
        last_error: last_error(engine),
    }
}

fn small_image() -> Vec<u8> {
    // a separate literal linked image isolates private publication from large-file parsing.
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
        (0xd0, 0x4000),
        (0xd4, 512),
        (0xf4, 16),
        (0x100, 0x3000),
        (0x104, 40),
        (0x158, 0x3080),
        (0x15c, 8),
    ] {
        put32(&mut bytes, at, value);
    }
    for (at, rva, virtual_size, raw, flags) in [
        (0x178, 0x1000, 5, 0x200, 0x6000_0020),
        (0x1a0, 0x3000, 0x200, 0x400, 0x4000_0040),
    ] {
        for (field, value) in [
            (8, virtual_size),
            (12, rva),
            (16, 512),
            (20, raw),
            (36, flags),
        ] {
            put32(&mut bytes, at + field, value);
        }
    }
    bytes[0x200..0x205].copy_from_slice(&[0xb8, 42, 0, 0, 0]);
    for (at, value) in [
        (0x400, 0x3040),
        (0x40c, 0x30c0),
        (0x410, 0x3080),
        (0x440, 0x3100),
        (0x480, 0x3100),
    ] {
        put32(&mut bytes, at, value);
    }
    bytes[0x4c0..0x4cd].copy_from_slice(b"kernel32.dll\0");
    bytes[0x502..0x50f].copy_from_slice(b"GetLastError\0");
    bytes
}

#[test]
fn owned_copy_and_every_refusal_preserve_the_original_reserved_allocation() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let before = owners(&engine);
    let source: Vec<u8> = (0..5000)
        .map(|index| (index as u8).wrapping_mul(19))
        .collect();
    engine.begin_image_input(5000).unwrap();
    let reserved = input(&engine).unwrap();
    assert_eq!(reserved.total, 5000);
    assert!(reserved.bytes.is_empty());
    assert!(reserved.capacity >= 5000);
    for (start, end) in [(0, 4096), (4096, 4097), (4097, 5000)] {
        let mut chunk = source[start..end].to_vec();
        engine.append_image_input(start as u32, &chunk).unwrap();
        chunk.fill(0xff);
        let current = input(&engine).unwrap();
        assert_eq!(current.bytes, source[..end]);
        assert_eq!(current.pointer, reserved.pointer);
        assert_eq!(current.capacity, reserved.capacity);
        assert_eq!(owners(&engine), before);
    }
    let complete = input(&engine);
    for (offset, bytes) in [
        (0, b"x".as_slice()),
        (5000, b"x"),
        (u32::MAX, b"x"),
        (5000, b""),
    ] {
        assert_eq!(
            engine.append_image_input(offset, bytes),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(input(&engine), complete);
        assert_eq!(owners(&engine), before);
    }
    assert_eq!(
        engine.append_image_input(5000, &[0; 4097]),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(input(&engine), complete);
    assert_eq!(owners(&engine), before);
    engine.abort_image_input().unwrap();
    assert!(engine.image_input.is_none());
    assert_eq!(owners(&engine), before);
    engine.begin_image_input(3).unwrap();
    engine.append_image_input(0, b"bad").unwrap();
    assert_eq!(input(&engine).unwrap().bytes, b"bad");
}

#[test]
fn raw_arena_cancel_last_error_tokens_start_latch_and_version_history_do_not_authenticate_input() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 3).unwrap();
    engine.unmap(0x1000, 1).unwrap();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    seed_last_error(&mut engine);
    engine.call_token = u32::MAX;
    engine.image_started = true;
    for (index, byte) in engine.arena_mut().unwrap().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(11);
    }
    put32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
    let before = owners(&engine);
    engine.begin_image_input(3).unwrap();
    engine.append_image_input(0, b"bad").unwrap();
    let staged = input(&engine);
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, GATE),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(input(&engine), staged);
    assert_eq!(owners(&engine), before);
    engine.abort_image_input().unwrap();
    assert_eq!(owners(&engine), before);
    assert_eq!(
        engine.map(0x2000, 1, 3),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(owners(&engine), before);
}

#[test]
fn failed_candidate_keeps_private_identity_and_success_consumes_only_staging_and_publishes_image() {
    let mut engine = EngineInstance::new(4, KEY).unwrap();
    seed_last_error(&mut engine);
    engine.call_token = 73;
    engine.image_started = true;
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    engine.arena_mut().unwrap().fill(0xd3);
    put32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
    let before = owners(&engine);
    let bytes = small_image();
    engine.begin_image_input(bytes.len() as u32).unwrap();
    engine.append_image_input(0, &bytes).unwrap();
    let staged = input(&engine);
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(BASE, BASE + 0x2000),
        Err(HostError::Loader(LoadError::Malformed))
    );
    assert_eq!(input(&engine), staged);
    assert_eq!(owners(&engine), before);
    assert_eq!(
        engine.map(0x9000, 1, 3),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    let linked = engine.load_pe32_linked_v2_input_at(BASE, GATE).unwrap();
    assert!(engine.image_input.is_none());
    assert_ne!(
        engine.memory().unwrap().identity(),
        before.identity.unwrap()
    );
    assert_eq!(
        linked.image,
        ImageMetadata32 {
            image_base: BASE,
            image_size: 0x4000,
            entry_point: BASE + 0x1000,
            mapped_pages: 4
        }
    );
    assert_eq!(engine.image, Some(linked.image));
    let mut after = owners(&engine);
    after.identity = before.identity;
    after.pages = before.pages;
    after.image = before.image;
    assert_eq!(after, before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 4);
    let committed = owners(&engine);
    engine.abort_image_input().unwrap();
    assert_eq!(owners(&engine), committed);
}

#[test]
fn pending_and_callback_busy_precede_pristine_and_shape_while_abort_only_cleans_input() {
    for suspended in [false, true] {
        let mut engine = EngineInstance::new(1, KEY).unwrap();
        engine.begin_image_input(10).unwrap();
        engine.append_image_input(0, b"saved").unwrap();
        engine.map(0x9000, 1, 3).unwrap();
        if suspended {
            engine.callback = Some(callback());
        } else {
            engine.pending_call = Some(pending());
        }
        engine.arena_mut().unwrap().fill(0xb7);
        put32(engine.arena_mut().unwrap(), CANCEL_OFFSET, 1);
        let before = owners(&engine);
        let staged = input(&engine);
        assert_eq!(
            engine.begin_image_input(0),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.append_image_input(u32::MAX, &[]),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.load_pe32_linked_v2_input_at(0, 0),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.load_pe32_linked_v2_at(b"bad", 0, 0),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(input(&engine), staged);
        assert_eq!(owners(&engine), before);
        engine.abort_image_input().unwrap();
        engine.abort_image_input().unwrap();
        assert!(engine.image_input.is_none());
        assert_eq!(owners(&engine), before);
    }
}

#[test]
fn terminal_priority_retains_input_and_close_releases_it_before_closed_priority() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.begin_image_input(8).unwrap();
    engine.append_image_input(0, b"owned").unwrap();
    // native terminal/owner latch model; real terminal calls belong to the actual fixture.
    engine.pending_call = Some(pending());
    engine.callback = Some(callback());
    engine.exit_code = Some(0xf123_4567);
    let before = owners(&engine);
    let staged = input(&engine);
    assert_eq!(engine.begin_image_input(0), Err(HostError::ProcessExited));
    assert_eq!(
        engine.append_image_input(u32::MAX, &[]),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.abort_image_input(), Err(HostError::ProcessExited));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::ProcessExited)
    );
    assert_eq!(input(&engine), staged);
    assert_eq!(owners(&engine), before);
    engine.close();
    engine.close();
    assert!(engine.image_input.is_none());
    assert!(engine.memory.is_none());
    assert!(engine.pending_call.is_none());
    assert!(engine.callback.is_none());
    assert!(engine.dispatcher.is_none());
    assert_eq!(engine.exit_code, Some(0xf123_4567));
    assert_eq!(engine.arena(), before.arena);
    assert_eq!(engine.begin_image_input(0), Err(HostError::Closed));
    assert_eq!(
        engine.append_image_input(u32::MAX, &[]),
        Err(HostError::Closed)
    );
    assert_eq!(engine.abort_image_input(), Err(HostError::Closed));
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::Closed)
    );
    assert!(engine.image_input.is_none());
    assert_eq!(engine.arena(), before.arena);
}

#[test]
fn cleanup_preserves_current_stale_code_owners_and_generation_after_unmap() {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.begin_image_input(3).unwrap();
    engine.append_image_input(0, b"bad").unwrap();
    for address in [0x1000, 0x2000] {
        engine.map(address, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), &[0x90])
            .unwrap();
        engine.protect(address, 1, 5).unwrap();
    }
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET, 0x1000);
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET + 4, 1);
    engine.compile(1).unwrap();
    let current = engine.compile_resident(1).unwrap();
    engine
        .acknowledge_resident_installation(KEY, current.get(), 0)
        .unwrap();
    put32(engine.arena_mut().unwrap(), TRANSFER_OFFSET, 0x2000);
    let stale = engine.compile_resident(1).unwrap();
    engine
        .acknowledge_resident_installation(KEY, stale.get(), 1)
        .unwrap();
    let fresh = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 1)
        .unwrap();
    let old = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x2000), 1)
        .unwrap();
    engine.protect(0x2000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &[0x90])
        .unwrap();
    engine.protect(0x2000, 1, 5).unwrap();
    seed_last_error(&mut engine);
    let before = owners(&engine);
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let artifact_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let current_bytes = engine.resident_bytes(current.get()).unwrap().to_vec();
    let current_pointer = engine.resident_bytes(current.get()).unwrap().as_ptr();
    let registry = format!("{:?}", engine.resident);
    let installations = engine.resident_installations;
    let stale_error = Err(HostError::Resident(RegistryError::CodeInvalidated));
    assert_eq!(engine.guard_resident(KEY, stale.get()), stale_error);
    assert_eq!(engine.begin_image_input(0), Err(HostError::InvalidRequest));
    assert_eq!(
        engine.append_image_input(3, b"x"),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        engine.load_pe32_linked_v2_input_at(0, 0),
        Err(HostError::InvalidRequest)
    );
    engine.abort_image_input().unwrap();
    engine.abort_image_input().unwrap();
    assert!(engine.image_input.is_none());
    assert_eq!(owners(&engine), before);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), artifact_pointer);
    assert_eq!(engine.resident_bytes(current.get()).unwrap(), current_bytes);
    assert_eq!(
        engine.resident_bytes(current.get()).unwrap().as_ptr(),
        current_pointer
    );
    assert_eq!(format!("{:?}", engine.resident), registry);
    assert_eq!(engine.resident_installations, installations);
    assert_eq!(engine.guard_resident(KEY, stale.get()), stale_error);
    assert!(engine.memory().unwrap().is_code_current(&fresh));
    assert!(!engine.memory().unwrap().is_code_current(&old));
    engine.unmap(0x1000, 1).unwrap();
    engine.unmap(0x2000, 1).unwrap();
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(engine.image, before.image);
    assert!(!engine.image_started);
    assert_eq!(engine.generation, before.generation);
    assert_eq!(engine.resident_installations, installations);
    assert_eq!(last_error(&engine), LAST_ERROR);
    assert_eq!(engine.arena(), before.arena);
}
