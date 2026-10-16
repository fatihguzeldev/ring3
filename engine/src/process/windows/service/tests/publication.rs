use super::support::*;
use crate::{
    abi::{arena::*, x86::*},
    memory::{GuestAddress, MemoryError},
    process::HostError,
    windows::{files::FileApi32, winmm::WinmmApi32},
};

#[test]
fn exhausted_guard_publication_preserves_frame_teb_last_error_and_exact_guard() {
    let mut engine = engine();
    put(&mut engine, DATA, b"C:\\Temp\\atomic.bin\0");
    let teb = engine.module_teb_address().unwrap();
    guard(&mut engine, teb, 1);
    let (generation, token) = capture(
        &mut engine,
        FileApi32::CreateFileA.id(),
        &[DATA, 0xc000_0000, 7, 0, 2, 0x80, 0],
    );
    let memory = engine.memory.as_mut().unwrap();
    let witness = memory.guard_page(GuestAddress(teb)).unwrap();
    let version = memory.transaction_version();
    memory.exhaust_versions_for_test();
    let before = engine.arena()[..EXIT_OFFSET + EXIT_SIZE].to_vec();
    assert_eq!(
        engine.complete_windows_call(KEY, generation, token),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(
        &engine.arena()[..EXIT_OFFSET + EXIT_SIZE],
        before.as_slice()
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert_eq!(engine.windows_thread.last_error(), 0x1234_5678);
    assert!(engine.windows_service.is_none());
    let memory = engine.memory.as_mut().unwrap();
    assert!(witness.is_current(memory));
    assert!(memory.guard_service_fault().is_none());
    let mut bytes = [0; 4];
    memory
        .read_after_private_changes(GuestAddress(teb + 0x34), &mut bytes, &[witness.clear()])
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 0x1234_5678);
    memory.restore_version_for_test(version);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert_eq!(result(&engine), u32::MAX);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
    assert_eq!(engine.memory().unwrap().guard_page(GuestAddress(teb)), None);
    assert!(engine.pending_call.is_none());
}

#[test]
fn guard_publication_keeps_multimedia_status_without_inventing_last_error() {
    let mut engine = engine();
    guard(&mut engine, DATA, 1);
    assert_eq!(
        call(&mut engine, WinmmApi32::TimeGetDevCaps.id(), &[DATA, 8]),
        97
    );
    assert_eq!(
        engine.current_windows_thread().unwrap().last_error(),
        0x1234_5678
    );
    assert_eq!(
        engine.memory().unwrap().guard_page(GuestAddress(DATA)),
        None
    );
    assert_eq!(
        call(&mut engine, WinmmApi32::TimeGetDevCaps.id(), &[DATA, 8]),
        0
    );
    assert_eq!(word(&engine, DATA), 1);
    assert_eq!(word(&engine, DATA + 4), 65535);
}
