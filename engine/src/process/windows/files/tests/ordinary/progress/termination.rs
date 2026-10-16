use super::{callback, setup, support};
use crate::{
    files::{FileAccess32, FileError32},
    process::windows::files::handles::KernelFileObject32,
    windows::{
        files::FileApi32,
        synchronization::{SynchronizationContext32, SynchronizationLimits32},
    },
};

#[test]
fn saved_copy_cleanup_preserves_other_thread_and_duplicate_owned_handles() {
    let (mut runtime, mut memory, _) = setup();
    let first = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0, 0, 0],
            )
            .unwrap(),
    );
    let source = runtime.handles.file(first.arguments[10]).unwrap();
    let destination = runtime.handles.file(first.arguments[11]).unwrap();
    let mut sync = SynchronizationContext32::new(7, SynchronizationLimits32::default(), 0).unwrap();
    let source_event = runtime.file_event(source, &mut sync).unwrap();
    let destination_event = runtime.file_event(destination, &mut sync).unwrap();
    runtime.vfs.retain_open(destination).unwrap();
    let duplicate = runtime.handles.prepare(FileAccess32::ALL.0, false).unwrap();
    let duplicate = runtime
        .handles
        .commit(duplicate, KernelFileObject32::File(destination));
    let lock = runtime.prepare_file_lock(destination, 0, 1, true).unwrap();
    runtime.commit_file_lock(lock);
    let saved = runtime.ordinary.take_thread_reply();

    support::path(&mut memory, 0x1100, "C:\\other-target.bin");
    let other = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0, 0, 0],
            )
            .unwrap(),
    );
    let revision = runtime.vfs.revision();
    let prepared = runtime.prepare_copy_thread_exit(&[&saved], false).unwrap();
    assert_eq!(runtime.vfs.revision(), revision);
    assert_eq!(prepared.closes(), &[(source, 2), (destination, 2)]);
    assert_eq!(prepared.event_handles(), &[source_event]);
    runtime
        .vfs
        .check_close_open_group(prepared.closes(), 0, &[])
        .unwrap();
    let close = sync
        .prepare_close_handles(prepared.event_handles())
        .unwrap();
    runtime.commit_copy_thread_exit(prepared);
    sync.commit_close_handles(close);

    assert!(runtime.handles.file(first.arguments[10]).is_err());
    assert!(runtime.handles.file(first.arguments[11]).is_err());
    assert!(runtime.vfs.open_info(source).is_err());
    assert_eq!(runtime.handles.file(duplicate), Ok(destination));
    assert_eq!(support::file_bytes(&mut runtime, duplicate), b"");
    assert_eq!(runtime.check_unlock_file_range(destination, 0, 1), Ok(()));
    assert!(runtime.events.contains(&(
        crate::windows::files::completion::CompletionTarget32::File(destination),
        destination_event
    )));
    assert!(!runtime.events.contains(&(
        crate::windows::files::completion::CompletionTarget32::File(source),
        source_event
    )));
    assert!(sync.validate_wait_handle(source_event).is_err());
    assert!(sync.validate_wait_handle(destination_event).is_ok());
    assert!(runtime.pending_copy_closes(other.continuation).is_ok());
}

#[test]
fn active_cleanup_removes_its_copy_and_duplicate_saved_owner_is_neutral() {
    let (mut runtime, mut memory, _) = setup();
    let copy = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0, 0, 0],
            )
            .unwrap(),
    );
    let saved = runtime.ordinary.take_thread_reply();
    let revision = runtime.vfs.revision();
    assert!(matches!(
        runtime.prepare_copy_thread_exit(&[&saved, &saved], false),
        Err(FileError32::InvalidParameter)
    ));
    assert_eq!(runtime.vfs.revision(), revision);
    assert!(runtime.handles.file(copy.arguments[10]).is_ok());
    runtime.ordinary.restore_thread_reply(saved);
    let prepared = runtime.prepare_copy_thread_exit(&[], true).unwrap();
    runtime
        .vfs
        .check_close_open_group(prepared.closes(), 0, &[])
        .unwrap();
    runtime.commit_copy_thread_exit(prepared);
    assert_eq!(
        runtime.pending_copy_closes(copy.continuation),
        Err(FileError32::InvalidHandle)
    );
    assert!(
        runtime
            .vfs
            .path_info(&"C:\\target.bin".encode_utf16().collect::<Vec<_>>())
            .is_ok()
    );
}
