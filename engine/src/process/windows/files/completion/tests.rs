use super::super::{ordinary::FileCallOutcome32, tests::support};
use crate::{
    files::{FileAccess32, FileDisposition32, FileError32, FileOpenOptions32},
    memory::GuestAddress,
    process::windows::threads::ThreadRuntime32,
    windows::{files::FileApi32 as Api, thread::ThreadLimits32},
};
fn result(outcome: FileCallOutcome32) -> (u32, Option<u32>) {
    let FileCallOutcome32::Return { value, last_error } = outcome else {
        panic!("completed call required")
    };
    (value, last_error)
}
#[test]
fn posted_packets_and_failed_io_packets_preserve_fifo_outputs_and_shared_description_key() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let threads = ThreadRuntime32::new(ThreadLimits32::default()).unwrap();
    let file = support::open_options(
        &mut files,
        "C:\\iocp",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::OpenAlways,
            overlapped: true,
            ..Default::default()
        },
    );
    let port = result(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::CreateIoCompletionPort,
                &[file, 0, 0xfeed, 1],
            )
            .unwrap(),
    )
    .0;
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::CreateIoCompletionPort,
                    &[file, port, 0xbeef, 0]
                )
                .unwrap()
        ),
        (port, None)
    );
    result(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::PostQueuedCompletionStatus,
                &[port, 11, 22, 0x33],
            )
            .unwrap(),
    );
    let open = files.handles.file(file).unwrap();
    let event = files.file_event(open, &mut sync).unwrap();
    memory.write(GuestAddress(0x1100), &[0; 20]).unwrap();
    memory.write(GuestAddress(0x1200), b"written").unwrap();
    assert_eq!(
        result(
            files
                .execute_overlapped(
                    &mut memory,
                    &mut sync,
                    1,
                    event,
                    Api::WriteFile,
                    &[file, 0x1200, 7, 0, 0x1100]
                )
                .unwrap()
        ),
        (0, Some(997))
    );
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    for (expected_bytes, expected_key, expected_pointer) in [(11, 22, 0x33), (7, 0xfeed, 0x1100)] {
        assert_eq!(
            result(
                files
                    .execute_completion_api(
                        &mut memory,
                        &threads,
                        0,
                        1,
                        Api::GetQueuedCompletionStatus,
                        &[port, 0x1300, 0x1304, 0x1308, 0]
                    )
                    .unwrap()
            ),
            (1, None)
        );
        assert_eq!(support::dword(&memory, 0x1300), expected_bytes);
        assert_eq!(support::dword(&memory, 0x1304), expected_key);
        assert_eq!(support::dword(&memory, 0x1308), expected_pointer);
    }
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (0, Some(258))
    );
    assert_eq!(support::dword(&memory, 0x1308), 0);
    assert_eq!(support::dword(&memory, 0x1300), 7);
    let mut overlap = [0; 20];
    overlap[16..].copy_from_slice(&1u32.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            event,
            Api::WriteFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (0, Some(258))
    );
    memory.write(GuestAddress(0x1100), &[0; 20]).unwrap();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            event,
            Api::ReadFile,
            &[file, 0x1200, 1, 0, 0x1100],
        )
        .unwrap();
    files
        .execute_async(
            &mut memory,
            Some(&mut sync),
            1,
            0,
            Api::CancelIoEx,
            &[file, 0x1100],
        )
        .unwrap();
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (0, Some(995))
    );
    assert_eq!(support::dword(&memory, 0x1308), 0x1100);
    assert_eq!(support::dword(&memory, 0x1304), 0xfeed);
}
#[test]
fn failed_output_keeps_packet_queued_and_port_close_wakes_exact_parked_thread_bundle() {
    let (mut files, mut memory, _) = support::runtime();
    let threads = ThreadRuntime32::new(ThreadLimits32::default()).unwrap();
    let port = result(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::CreateIoCompletionPort,
                &[u32::MAX, 0, 0, 1],
            )
            .unwrap(),
    )
    .0;
    files
        .execute_completion_api(
            &mut memory,
            &threads,
            0,
            1,
            Api::PostQueuedCompletionStatus,
            &[port, 9, 8, 7],
        )
        .unwrap();
    assert_eq!(
        files.execute_completion_api(
            &mut memory,
            &threads,
            0,
            1,
            Api::GetQueuedCompletionStatus,
            &[port, 0x1300, 0x1304, 0x8000, 0]
        ),
        Err(FileError32::NoAccess)
    );
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatusEx,
                    &[port, 0x1400, 2, 0x1300, 0, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(
        support::read::<16>(&memory, 0x1400),
        [8, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0]
    );
    assert_eq!(support::dword(&memory, 0x1300), 1);
    assert_eq!(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::GetQueuedCompletionStatus,
                &[port, 0x1300, 0x1304, 0x1308, u32::MAX]
            )
            .unwrap(),
        FileCallOutcome32::Pending
    );
    let reply = files.take_thread_reply();
    assert!(files.completion_wait.is_none());
    files.close_kernel_handle(&mut memory, None, port).unwrap();
    files.restore_thread_reply(reply);
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    1,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, u32::MAX]
                )
                .unwrap()
        ),
        (0, Some(735))
    );
    assert_eq!(support::dword(&memory, 0x1308), 0);
}

#[test]
fn immediate_lock_posts_packet_and_notification_modes_keep_explicit_event_independent() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let threads = ThreadRuntime32::new(ThreadLimits32::default()).unwrap();
    let file = support::open_options(
        &mut files,
        "C:\\lock-completion",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::OpenAlways,
            overlapped: true,
            ..Default::default()
        },
    );
    let port = result(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::CreateIoCompletionPort,
                &[file, 0, 0x5432, 1],
            )
            .unwrap(),
    )
    .0;
    let open = files.handles.file(file).unwrap();
    let private = files.file_event(open, &mut sync).unwrap();
    let (event, _) = sync.create_event(true, false, false, None).unwrap();
    let mut overlap = [0; 20];
    overlap[16..].copy_from_slice(&event.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    assert_eq!(
        result(
            files
                .execute_lock_ex(
                    &mut memory,
                    Some(&mut sync),
                    1,
                    Api::LockFileEx,
                    &[file, 2, 0, 2, 0, 0x1100]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::dword(&memory, 0x1304), 0x5432);
    files
        .execute_lock_ex(
            &mut memory,
            Some(&mut sync),
            1,
            Api::UnlockFileEx,
            &[file, 0, 2, 0, 0x1100],
        )
        .unwrap();
    files
        .execute_completion_api(
            &mut memory,
            &threads,
            0,
            1,
            Api::SetFileCompletionNotificationModes,
            &[file, 2],
        )
        .unwrap();
    sync.reset_event(private).unwrap();
    sync.reset_event(event).unwrap();
    overlap[8..12].copy_from_slice(&4u32.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    files
        .execute_lock_ex(
            &mut memory,
            Some(&mut sync),
            1,
            Api::LockFileEx,
            &[file, 2, 0, 2, 0, 0x1100],
        )
        .unwrap();
    assert!(!sync.event_is_signaled(private).unwrap());
    assert!(sync.event_is_signaled(event).unwrap());
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    files
        .execute_lock_ex(
            &mut memory,
            Some(&mut sync),
            1,
            Api::UnlockFileEx,
            &[file, 0, 2, 0, 0x1100],
        )
        .unwrap();
    files
        .execute_completion_api(
            &mut memory,
            &threads,
            0,
            1,
            Api::SetFileCompletionNotificationModes,
            &[file, 1],
        )
        .unwrap();
    overlap[8..12].copy_from_slice(&8u32.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    files
        .execute_lock_ex(
            &mut memory,
            Some(&mut sync),
            1,
            Api::LockFileEx,
            &[file, 2, 0, 2, 0, 0x1100],
        )
        .unwrap();
    assert_eq!(
        result(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1300, 0x1304, 0x1308, 0]
                )
                .unwrap()
        ),
        (0, Some(258))
    );
}
