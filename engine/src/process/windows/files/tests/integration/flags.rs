use super::*;
use crate::windows::{WindowsApi32, process::ProcessApi32};
#[test]
fn file_and_event_protection_matches_native_close_and_duplicate_close_source_behavior() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    let event = complete(&mut engine, WindowsApi32::CreateEventW.id(), &[0, 0, 0, 0]);
    for handle in [file, event] {
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::SetHandleInformation.id(),
                &[handle, 3, 3]
            ),
            1
        );
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::GetHandleInformation.id(),
                &[handle, 0x2000]
            ),
            1
        );
        assert_eq!(word(&engine, 0x2000), 3);
        assert_eq!(
            complete(&mut engine, WindowsApi32::CloseHandle.id(), &[handle]),
            0
        );
        assert_eq!(
            complete(&mut engine, WindowsApi32::GetLastError.id(), &[]),
            6
        );
        assert_eq!(
            complete(
                &mut engine,
                WindowsApi32::DuplicateHandle.id(),
                &[u32::MAX, handle, u32::MAX, 0x2004, 0, 0, 3]
            ),
            1
        );
        let copy = word(&engine, 0x2004);
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::GetHandleInformation.id(),
                &[handle, 0x2000]
            ),
            1
        );
        assert_eq!(word(&engine, 0x2000), 3);
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::GetHandleInformation.id(),
                &[copy, 0x2000]
            ),
            1
        );
        assert_eq!(word(&engine, 0x2000), 0);
        assert_eq!(
            complete(&mut engine, WindowsApi32::CloseHandle.id(), &[copy]),
            1
        );
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::SetHandleInformation.id(),
                &[handle, 2, 0]
            ),
            1
        );
        assert_eq!(
            complete(
                &mut engine,
                ProcessApi32::GetHandleInformation.id(),
                &[handle, 0x2000]
            ),
            1
        );
        assert_eq!(word(&engine, 0x2000), 1);
        assert_eq!(
            complete(&mut engine, WindowsApi32::CloseHandle.id(), &[handle]),
            1
        );
    }
    assert_eq!(
        complete(
            &mut engine,
            ProcessApi32::GetHandleInformation.id(),
            &[u32::MAX, 0x2000]
        ),
        1
    );
    assert_eq!(word(&engine, 0x2000), 0);
}
fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
