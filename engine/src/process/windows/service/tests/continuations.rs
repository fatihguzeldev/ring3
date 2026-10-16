use super::support::*;
use crate::{
    files::{AssetRef32, FileTimes32},
    memory::{GuestAddress, MemoryError},
    process::HostError,
    windows::{
        files::FileApi32,
        user32::{Message32, Point32, User32Api32, User32Error32},
    },
};

#[test]
fn resumed_lazy_file_read_clears_guard_without_consuming_the_file_cursor() {
    let mut engine = engine();
    engine
        .mount_file(
            &"C:\\guard-source.bin".encode_utf16().collect::<Vec<_>>(),
            AssetRef32 { id: 23, length: 1 },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    put(&mut engine, DATA, b"C:\\guard-source.bin\0");
    let handle = call(
        &mut engine,
        FileApi32::CreateFileA.id(),
        &[DATA, 0x8000_0000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(handle, u32::MAX);
    let args = [handle, DATA + 0x1000, 1, DATA + 0x2000, 0];
    let (generation, token) = capture(&mut engine, FileApi32::ReadFile.id(), &args);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    assert!(engine.pending_file_call().is_some());
    let request = engine.pending_file_request().unwrap().unwrap();
    engine.supply_file_bytes(request, 0, &[9]).unwrap();
    guard(&mut engine, DATA + 0x1000, 1);
    engine.resume_file_call(KEY, token).unwrap();
    assert_eq!(result(&engine), 0);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
    assert!(engine.pending_file_call().is_none());
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(DATA + 0x1000))
            .is_none()
    );
    assert_eq!(call(&mut engine, FileApi32::ReadFile.id(), &args), 1);
    assert_eq!(word(&engine, DATA + 0x2000), 1);
    assert_eq!(word(&engine, DATA + 0x1000), 9);
}

#[test]
fn parked_getmessage_keeps_its_message_and_owner_when_guard_publication_fails() {
    let mut engine = engine();
    engine
        .configure_user32(Default::default(), range(0x3200_0000, 16))
        .unwrap();
    engine.register_user32_module(0x7400_0000).unwrap();
    engine.load_library_module("user32.dll", None).unwrap();
    let (generation, token) = capture(&mut engine, User32Api32::GetMessageA.id(), &[DATA, 0, 0, 0]);
    engine
        .complete_windows_call(KEY, generation, token)
        .unwrap();
    let continuation = engine.waiting_user32_message().unwrap();
    engine
        .post_user32_message(Message32 {
            window: 0,
            message: 0x8000,
            wparam: 42,
            lparam: 17,
            time_ms: 0,
            point: Point32 { x: 1, y: 2 },
            thread_id: 1,
        })
        .unwrap();
    guard(&mut engine, DATA, 1);
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert_eq!(
        engine.resume_user32_wait(KEY, continuation),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(engine.waiting_user32_message(), Some(continuation));
    assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(DATA))
            .is_some()
    );
    assert!(engine.windows_service.is_none());
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    assert!(matches!(
        engine.resume_user32_wait(KEY, continuation),
        Err(HostError::User32(User32Error32::Memory(_)))
    ));
    assert_eq!(engine.waiting_user32_message(), Some(continuation));
    assert!(
        engine
            .memory()
            .unwrap()
            .guard_page(GuestAddress(DATA))
            .is_none()
    );
    engine.resume_user32_wait(KEY, continuation).unwrap();
    assert_eq!(result(&engine), 1);
    assert_eq!(word(&engine, DATA + 4), 0x8000);
    assert_eq!(word(&engine, DATA + 8), 42);
    assert!(engine.waiting_user32_message().is_none());
    assert!(engine.pending_call.is_none());
    assert_eq!(
        call(
            &mut engine,
            User32Api32::PeekMessageA.id(),
            &[DATA, 0, 0, 0, 1]
        ),
        0
    );
}
