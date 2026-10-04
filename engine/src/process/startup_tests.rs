use super::*;
use crate::{
    cpu::dbt::{RegistryError, RegistryUsage},
    loader::ImageMetadata32,
    memory::{Access, MemoryError},
    process::ResidentInstallation,
    windows::{CallFrame32, CallingConvention32, WindowsApi32, WindowsOutcome32},
};

const KEY: u64 = 0x1020_3040_5060_7080;
const BASE: u32 = 0x0040_0000;
const ENTRY: u32 = BASE + 4096;
const STACK: u32 = 0x8000;
const OTHER_CODE: u32 = 0x6000_0000;
const OTHER_RAM: u32 = 0x7000_0000;

fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}

fn fixture(pages: u32) -> EngineInstance {
    // the public literal pe tests own parsing; this fixture isolates private publication.
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    engine.map(BASE, 1, 1).unwrap();
    engine.map(ENTRY, 1, 7).unwrap();
    write(&mut engine, ENTRY, &[0x90, 0xeb, 0]);
    engine.protect(ENTRY, 1, 5).unwrap();
    engine.image = Some(ImageMetadata32 {
        image_base: BASE,
        image_size: 0x4000,
        entry_point: ENTRY,
        mapped_pages: 2,
    });
    engine
}

fn entry(engine: &mut EngineInstance, pc: u32) {
    engine.arena_mut().unwrap()[140..144].copy_from_slice(&pc.to_le_bytes());
}

fn frame(engine: &EngineInstance, words: u32) -> CallFrame32 {
    let mut state = State32::default();
    state.registers[4] = OTHER_RAM;
    CallFrame32::capture(
        engine.memory().unwrap(),
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
            &frame(engine, 0),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap()
        .0
    {
        WindowsOutcome32::Return(value) => value,
        WindowsOutcome32::ExitProcess(_) => panic!("GetLastError cannot terminate the process"),
        WindowsOutcome32::Allocate { .. } => panic!("getlasterror must return"),
    }
}

fn retained_fixture() -> (EngineInstance, [u64; 2]) {
    let mut engine = fixture(8);
    engine.map(OTHER_CODE, 1, 7).unwrap();
    engine.map(OTHER_RAM, 1, 3).unwrap();
    write(&mut engine, OTHER_CODE, &[0x90, 0xeb, 0]);
    write(&mut engine, OTHER_RAM, &ENTRY.to_le_bytes());
    write(&mut engine, OTHER_RAM + 4, &0xf123_4567_u32.to_le_bytes());
    let (_, thread) = engine
        .windows_thread
        .prepare(
            WindowsApi32::SetLastError,
            &frame(&engine, 1),
            crate::windows::ProcessContext32::default(),
        )
        .unwrap();
    engine.windows_thread = thread;
    entry(&mut engine, ENTRY);
    engine.compile_entries(1, 0).unwrap();
    let current = engine.compile_resident_entries(1, 0).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, current, 0)
        .unwrap();
    entry(&mut engine, OTHER_CODE);
    let stale = engine.compile_resident_entries(1, 0).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, stale, 1)
        .unwrap();
    // even a same-byte write makes the second unit stale; startup must not resurrect it.
    write(&mut engine, OTHER_CODE, &[0x90]);
    engine.arena_mut().unwrap()[100..].fill(0xa5);
    (engine, [current, stale])
}

#[derive(Debug, PartialEq, Eq)]
struct Owners {
    memory_identity: u64,
    image: Option<ImageMetadata32>,
    generation: u32,
    call_token: u32,
    arena_address: usize,
    artifact: Vec<u8>,
    artifact_pointer: usize,
    dispatcher: Vec<u8>,
    dispatcher_pointer: usize,
    usage: RegistryUsage,
    installations: Vec<Option<ResidentInstallation>>,
    current_bytes: Vec<u8>,
    current_pointer: usize,
    stale_result: Result<(), HostError>,
    last_error: u32,
    ram: Vec<u8>,
}

fn owners(engine: &EngineInstance, ids: [u64; 2]) -> Owners {
    let mut ram = vec![0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(OTHER_RAM), &mut ram)
        .unwrap();
    Owners {
        memory_identity: engine.memory().unwrap().identity(),
        image: engine.image,
        generation: engine.generation,
        call_token: engine.call_token,
        arena_address: engine.arena_address(),
        artifact: engine.artifact_bytes().unwrap().to_vec(),
        artifact_pointer: engine.artifact_bytes().unwrap().as_ptr() as usize,
        dispatcher: engine.dispatcher_bytes(KEY).unwrap().to_vec(),
        dispatcher_pointer: engine.dispatcher_bytes(KEY).unwrap().as_ptr() as usize,
        usage: engine.resident.as_ref().unwrap().usage(),
        installations: engine.resident_installations.to_vec(),
        current_bytes: engine.resident_bytes(ids[0]).unwrap().to_vec(),
        current_pointer: engine.resident_bytes(ids[0]).unwrap().as_ptr() as usize,
        stale_result: engine.guard_resident(KEY, ids[1]),
        last_error: last_error(engine),
        ram,
    }
}

#[test]
fn startup_keeps_same_memory_private_image_current_and_stale_owners_and_last_error() {
    let (mut engine, ids) = retained_fixture();
    let before = owners(&engine, ids);
    assert_eq!(before.usage.units, 2);
    assert_eq!(before.last_error, 0xf123_4567);
    assert_eq!(
        before.stale_result,
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    let current = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(ENTRY), 3)
        .unwrap();
    let stale = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(OTHER_CODE), 3)
        .unwrap();
    write(&mut engine, OTHER_CODE, &[0x90]);
    assert!(!engine.memory().unwrap().is_code_current(&stale));
    let untouched = engine.arena()[96..].to_vec();
    assert!(!engine.image_started);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 4);
    engine.start_loaded_image(STACK, 2).unwrap();
    assert_eq!(owners(&engine, ids), before);
    assert_eq!(&engine.arena()[96..], untouched);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 6);
    assert!(engine.image_started);
    assert!(engine.memory().unwrap().is_code_current(&current));
    assert!(!engine.memory().unwrap().is_code_current(&stale));
    let mut stack = [0xff; 8192];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(STACK), &mut stack)
        .unwrap();
    assert_eq!(stack, [0; 8192]);
}

#[test]
fn exhausted_mapping_version_keeps_arena_ram_identity_image_latch_and_all_retained_owners() {
    let (mut engine, ids) = retained_fixture();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    let before = owners(&engine, ids);
    let arena = engine.arena().to_vec();
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(ENTRY), 3)
        .unwrap();
    assert_eq!(
        engine.start_loaded_image(STACK, 2),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(owners(&engine, ids), before);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 4);
    assert!(engine.memory().unwrap().is_code_current(&code));
    assert!(!engine.image_started);
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(STACK), Access::Read)
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .resolve(GuestAddress(STACK + 4096), Access::Read)
            .is_err()
    );
    // no partial map or post-map write consumed a different version path.
    assert_eq!(
        engine.map(STACK, 1, 3),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.arena(), arena);
}

#[test]
fn collision_and_capacity_keep_private_identity_and_latch_until_retry_commits() {
    for collision in [false, true] {
        let mut engine = fixture(4);
        if collision {
            engine.map(STACK + 4096, 1, 3).unwrap();
            write(&mut engine, STACK + 4096, &[0xa7]);
        }
        let identity = engine.memory().unwrap().identity();
        let image = engine.image;
        let arena = engine.arena().to_vec();
        let code = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(ENTRY), 3)
            .unwrap();
        let error = if collision {
            MemoryError::AlreadyMapped {
                address: GuestAddress(STACK + 4096),
            }
        } else {
            MemoryError::Capacity
        };
        assert_eq!(
            engine.start_loaded_image(STACK, 3),
            Err(HostError::Memory(error))
        );
        assert_eq!(engine.memory().unwrap().identity(), identity);
        assert_eq!(engine.image, image);
        assert_eq!(engine.arena(), arena);
        assert!(!engine.image_started);
        assert!(engine.memory().unwrap().is_code_current(&code));
        if collision {
            let mut marker = [0];
            engine
                .memory()
                .unwrap()
                .read(GuestAddress(STACK + 4096), &mut marker)
                .unwrap();
            assert_eq!(marker, [0xa7]);
            engine.unmap(STACK + 4096, 1).unwrap();
        }
        engine.start_loaded_image(STACK, 2).unwrap();
        assert_eq!(engine.memory().unwrap().identity(), identity);
        assert_eq!(engine.image, image);
        assert!(engine.image_started);
        assert!(engine.memory().unwrap().is_code_current(&code));
    }
}

#[test]
fn historical_private_call_tokens_reject_even_exact_initial_context() {
    for token in [1, u32::MAX] {
        let mut engine = fixture(3);
        engine.call_token = token;
        let arena = engine.arena().to_vec();
        let identity = engine.memory().unwrap().identity();
        assert_eq!(
            engine.start_loaded_image(1, u32::MAX),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.call_token, token);
        assert_eq!(engine.memory().unwrap().identity(), identity);
        assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
        assert_eq!(engine.arena(), arena);
        assert!(!engine.image_started);
    }
}

#[test]
fn private_start_latch_survives_manual_restore_and_unmap_and_closed_still_wins() {
    let mut engine = fixture(3);
    let arena = engine.arena().to_vec();
    let identity = engine.memory().unwrap().identity();
    let image = engine.image;
    engine.start_loaded_image(STACK, 1).unwrap();
    engine.arena_mut().unwrap().copy_from_slice(&arena);
    engine.unmap(STACK, 1).unwrap();
    assert!(engine.image_started);
    assert_eq!(
        engine.start_loaded_image(STACK + 4096, 1),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.memory().unwrap().identity(), identity);
    assert_eq!(engine.image, image);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 2);
    engine.close();
    assert_eq!(
        engine.start_loaded_image(1, u32::MAX),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), arena);
    assert!(engine.memory.is_none());
    assert!(engine.artifact.is_none());
    assert!(engine.resident.is_none());
    assert!(engine.resident_installations.iter().all(Option::is_none));
}
