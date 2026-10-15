use super::*;
use crate::{
    process::windows::files::tests::support,
    windows::{
        WindowsApi32 as Api,
        synchronization::{WAIT_FAILED, WAIT_TIMEOUT},
    },
};

#[test]
fn invalid_mixed_wait_returns_guest_error_without_creating_partial_hidden_events() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1200),
            &[file.to_le_bytes(), 0x8000_fffcu32.to_le_bytes()].concat(),
        )
        .unwrap();
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1200, 0, 0]
        ),
        WAIT_FAILED
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert!(engine.files.as_ref().unwrap().events.is_empty());
    let event = complete(&mut engine, Api::CreateEventW.id(), &[0, 1, 0, 0]);
    assert_eq!(
        complete(
            &mut engine,
            Api::SignalObjectAndWait.id(),
            &[event, 0x8000_fffc, 0, 0]
        ),
        WAIT_FAILED
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert!(
        !engine
            .synchronization()
            .unwrap()
            .event_is_signaled(event)
            .unwrap()
    );
    assert_eq!(
        complete(
            &mut engine,
            Api::SignalObjectAndWait.id(),
            &[event, file, 0, 0]
        ),
        0
    );
    assert!(
        engine
            .synchronization()
            .unwrap()
            .event_is_signaled(event)
            .unwrap()
    );
}

#[test]
fn distinct_file_duplicate_aliases_wait_any_and_narrowed_rights_are_preserved() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, file, u32::MAX, 0x1200, 0, 0, 2]
        ),
        1
    );
    let duplicate = support::dword(engine.memory().unwrap(), 0x1200);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1300),
            &[file.to_le_bytes(), duplicate.to_le_bytes()].concat(),
        )
        .unwrap();
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1300, 0, 0]
        ),
        0
    );
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1300, 1, 0]
        ),
        WAIT_FAILED
    );
    assert_eq!(engine.windows_thread.last_error(), 87);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, file, u32::MAX, 0x1200, 1, 0, 0]
        ),
        1
    );
    let narrow = support::dword(engine.memory().unwrap(), 0x1200);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[narrow, 0]),
        WAIT_FAILED
    );
    assert_eq!(engine.windows_thread.last_error(), 5);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[duplicate, 0]),
        0
    );
    let event = complete(&mut engine, Api::CreateEventW.id(), &[0, 0, 0, 0]);
    assert_eq!(
        complete(&mut engine, Api::WaitForSingleObject.id(), &[event, 0]),
        WAIT_TIMEOUT
    );
}

#[test]
fn duplicate_close_source_closes_on_output_error_and_null_output_allocates_a_duplicate() {
    let mut engine = engine();
    let first = file(&mut engine, false);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, first, u32::MAX, 0x9000, 0, 0, 3]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 998);
    assert!(
        engine
            .files
            .as_ref()
            .unwrap()
            .handles
            .object(first)
            .is_err()
    );
    let second = file(&mut engine, false);
    let open = engine.files.as_ref().unwrap().handles.file(second).unwrap();
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, second, u32::MAX, 0, 0, 0, 2]
        ),
        1
    );
    assert_eq!(engine.files.as_ref().unwrap().handles.file_count(open), 2);
}

#[test]
fn rejected_async_read_does_not_retain_a_private_file_event() {
    use crate::windows::files::FileApi32;
    let mut engine = engine_with_events(1);
    let file = file(&mut engine, true);
    let before = support::read::<8>(engine.memory().unwrap(), 0x1100);
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::ReadFile.id(),
            &[file, 0x9000, 1, 0, 0x1100]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 998);
    assert_eq!(support::read::<8>(engine.memory().unwrap(), 0x1100), before);
    assert!(engine.files.as_ref().unwrap().events.is_empty());
    assert_ne!(
        complete(&mut engine, Api::CreateEventW.id(), &[0, 1, 0, 0]),
        0
    );
}

#[test]
fn rejected_lock_event_does_not_retain_a_private_event_or_publish_the_lock() {
    use crate::windows::files::FileApi32;
    let mut engine = engine_with_events(1);
    let file = file(&mut engine, true);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1110), &0x1234u32.to_le_bytes())
        .unwrap();
    let before = support::read::<8>(engine.memory().unwrap(), 0x1100);
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::LockFileEx.id(),
            &[file, 2, 0, 1, 0, 0x1100]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert_eq!(support::read::<8>(engine.memory().unwrap(), 0x1100), before);
    let files = engine.files.as_ref().unwrap();
    assert!(files.events.is_empty());
    assert!(
        files
            .prepare_file_lock(files.handles.file(file).unwrap(), 0, 1, true)
            .is_ok()
    );
    assert_ne!(
        complete(&mut engine, Api::CreateEventW.id(), &[0, 1, 0, 0]),
        0
    );
}

#[test]
fn first_wait_all_on_file_aliases_releases_only_new_private_events_on_rejection() {
    let mut engine = engine_with_events(1);
    let file = file(&mut engine, false);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, file, u32::MAX, 0x1200, 0, 0, 2]
        ),
        1
    );
    let duplicate = support::dword(engine.memory().unwrap(), 0x1200);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1300),
            &[file.to_le_bytes(), duplicate.to_le_bytes()].concat(),
        )
        .unwrap();
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1300, 1, 0]
        ),
        WAIT_FAILED
    );
    assert_eq!(engine.windows_thread.last_error(), 87);
    assert!(engine.files.as_ref().unwrap().events.is_empty());
    let event = complete(&mut engine, Api::CreateEventW.id(), &[0, 1, 0, 0]);
    assert_ne!(event, 0);
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[event]), 1);
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1300, 0, 0]
        ),
        0
    );
    let accepted = engine.files.as_ref().unwrap().events.clone();
    assert_eq!(
        complete(
            &mut engine,
            Api::WaitForMultipleObjects.id(),
            &[2, 0x1300, 1, 0]
        ),
        WAIT_FAILED
    );
    assert_eq!(engine.files.as_ref().unwrap().events, accepted);
}
