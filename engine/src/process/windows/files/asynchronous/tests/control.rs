use super::{result, setup};
use crate::{
    memory::GuestAddress,
    process::windows::files::{
        asynchronous::AsyncProgress32, ordinary::FileCallOutcome32, tests::support::*,
    },
    windows::files::FileApi32 as Api,
};

#[test]
fn cancel_io_is_thread_scoped_and_cancel_io_ex_completes_exact_operation() {
    let (mut files, mut memory, mut sync, file, event, file_event) = setup();
    memory.write(GuestAddress(0x1200), &[8]).unwrap();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::WriteFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    assert_eq!(
        result(
            files
                .execute_async(&mut memory, None, 2, 0, Api::CancelIo, &[file])
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(
        result(
            files
                .execute_async(&mut memory, None, 2, 0, Api::CancelIoEx, &[file, 0x1120])
                .unwrap()
        ),
        (0, Some(1168))
    );
    assert_eq!(
        result(
            files
                .execute_async(&mut memory, None, 2, 0, Api::CancelIoEx, &[file, 0x1100])
                .unwrap()
        ),
        (1, None)
    );
    assert!(!sync.event_is_signaled(event).unwrap());
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Completed {
            error: 995,
            bytes: 0,
            ..
        }
    ));
    assert_eq!(file_bytes(&mut files, file), [1, 2, 3]);
    assert_eq!(dword(&memory, 0x1100), 0xc000_0120);
    assert!(sync.event_is_signaled(event).unwrap());
}

#[test]
fn overlapped_result_ex_keeps_original_timeout_across_resume() {
    let (mut files, mut memory, mut sync, file, _, file_event) = setup();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::ReadFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    let args = [file, 0x1100, 0x1300, 50, 0];
    assert!(matches!(
        files
            .execute_async(
                &mut memory,
                None,
                1,
                10_000_000,
                Api::GetOverlappedResultEx,
                &args
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    assert!(matches!(
        files
            .execute_async(
                &mut memory,
                None,
                1,
                40_000_000,
                Api::GetOverlappedResultEx,
                &args
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    assert_eq!(
        result(
            files
                .execute_async(
                    &mut memory,
                    None,
                    1,
                    60_000_000,
                    Api::GetOverlappedResultEx,
                    &args
                )
                .unwrap()
        ),
        (0, Some(258))
    );
}

#[test]
fn invalid_result_output_never_consumes_completed_operation() {
    let (mut files, mut memory, mut sync, file, _, file_event) = setup();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::ReadFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert!(
        files
            .execute_async(
                &mut memory,
                None,
                1,
                0,
                Api::GetOverlappedResult,
                &[file, 0x1100, 0x9000, 0]
            )
            .is_err()
    );
    assert_eq!(
        result(
            files
                .execute_async(
                    &mut memory,
                    None,
                    1,
                    0,
                    Api::GetOverlappedResult,
                    &[file, 0x1100, 0x1300, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(dword(&memory, 0x1300), 1);
}

#[test]
fn unbuffered_async_and_positioned_calls_reject_unaligned_io_before_event_or_memory_changes() {
    use crate::files::{FileAccess32, FileDisposition32, FileOpenOptions32};
    let (mut files, mut memory, mut sync, _, _, _) = setup();
    let file = open_options(
        &mut files,
        "C:\\aligned.bin",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::OpenAlways,
            overlapped: true,
            no_buffering: true,
            ..FileOpenOptions32::default()
        },
    );
    write_file(&mut files, file, &vec![0; 512]);
    let open = files.handles.file(file).unwrap();
    let event = files.file_event(open, &mut sync).unwrap();
    let before = memory.transaction_version();
    let raw = read::<8>(&memory, 0x1100);
    assert!(matches!(
        files.execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            event,
            Api::ReadFile,
            &[file, 0x1201, 512, 0, 0x1100]
        ),
        Err(crate::files::FileError32::InvalidParameter)
    ));
    assert_eq!(memory.transaction_version(), before);
    assert_eq!(read::<8>(&memory, 0x1100), raw);
    assert!(sync.event_is_signaled(event).unwrap());
    assert!(matches!(
        files.execute_positioned(&mut memory, Api::ReadFile, &[file, 0x1200, 511, 0, 0x1100]),
        Err(crate::files::FileError32::InvalidParameter)
    ));
    assert_eq!(files.vfs.position(open).unwrap(), 0);
}

#[test]
fn completed_event_leases_are_released_and_evicted_status_keeps_lock_error() {
    use crate::files::{FileAccess32, FileDisposition32, FileOpenOptions32};
    let (mut files, mut memory, mut sync, file, event, file_event) = setup();
    files.asynchronous.max_operations = 1;
    let other = open_options(
        &mut files,
        "C:\\async.bin",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::OpenExisting,
            ..FileOpenOptions32::default()
        },
    );
    let owner = files.handles.file(other).unwrap();
    let lock = files.prepare_file_lock(owner, 0, 1, true).unwrap();
    files.commit_file_lock(lock);
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::ReadFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Completed { error: 33, .. }
    ));
    assert!(files.asynchronous.operations[0].events.is_empty());
    sync.close_handle(event).unwrap();
    files.unlock_file_range(owner, 0, 1).unwrap();
    memory.write(GuestAddress(0x1140), &[0; 20]).unwrap();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::ReadFile,
            &[file, 0x1200, 1, 0, 0x1140],
        )
        .unwrap();
    assert_eq!(
        result(
            files
                .execute_async(
                    &mut memory,
                    None,
                    1,
                    0,
                    Api::GetOverlappedResult,
                    &[file, 0x1100, 0x1300, 0]
                )
                .unwrap()
        ),
        (0, Some(33))
    );
}
