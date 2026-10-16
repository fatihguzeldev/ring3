use super::super::{FileCallOutcome32, tests::support};
use crate::{
    files::FileError32,
    memory::GuestAddress,
    process::windows::threads::ThreadRuntime32,
    windows::{files::FileApi32 as Api, security::Token32, thread::ThreadLimits32},
};

fn returned(outcome: FileCallOutcome32) -> (u32, Option<u32>) {
    match outcome {
        FileCallOutcome32::Return { value, last_error } => (value, last_error),
        _ => panic!("completed pipe call expected"),
    }
}
#[test]
fn named_overlapped_connect_partial_messages_and_cancel_share_the_real_completion_port() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let token = Token32::virtual_process();
    let threads = ThreadRuntime32::new(ThreadLimits32::default()).unwrap();
    support::path(&mut memory, 0x1000, r"\\.\pipe\literal");
    let server = returned(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: &token,
                },
                Api::CreateNamedPipeW,
                &[0x1000, 0x40000003, 6, 1, 16, 16, 0, 0],
            )
            .unwrap(),
    )
    .0;
    let port = returned(
        files
            .execute_completion_api(
                &mut memory,
                &threads,
                0,
                1,
                Api::CreateIoCompletionPort,
                &[server, 0, 0x4567, 1],
            )
            .unwrap(),
    )
    .0;
    memory.write(GuestAddress(0x1100), &[0; 20]).unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ConnectNamedPipe,
                    &[server, 0x1100]
                )
                .unwrap()
        ),
        (0, Some(997))
    );
    let name: Vec<u16> = r"\\.\pipe\literal".encode_utf16().collect();
    let client = returned(
        files
            .open_pipe_client(&memory, &name, &[0x1000, 0xc0000000, 0, 0, 3, 0, 0], &token)
            .unwrap(),
    )
    .0;
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(support::dword(&memory, 0x1100), 0);
    assert_eq!(
        returned(
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
    assert_eq!(support::dword(&memory, 0x1304), 0x4567);
    assert_eq!(support::dword(&memory, 0x1308), 0x1100);
    memory.write(GuestAddress(0x1200), b"abcde").unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 2,
                        now: 0,
                        token: &token
                    },
                    Api::WriteFile,
                    &[client, 0x1200, 5, 0x1310, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    memory.write(GuestAddress(0x1100), &[0; 20]).unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ReadFile,
                    &[server, 0x1400, 2, 0, 0x1100]
                )
                .unwrap()
        ),
        (0, Some(997))
    );
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(support::read::<2>(&memory, 0x1400), *b"ab");
    assert_eq!(support::dword(&memory, 0x1104), 2);
    assert_eq!(
        returned(
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
        (0, Some(234))
    );
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 1,
                now: 0,
                token: &token,
            },
            Api::PeekNamedPipe,
            &[server, 0x1400, 3, 0x1310, 0x1314, 0x1318],
        )
        .unwrap();
    assert_eq!(support::read::<3>(&memory, 0x1400), *b"cde");
    assert_eq!(support::dword(&memory, 0x1314), 3);
    memory.write(GuestAddress(0x1100), &[0; 20]).unwrap();
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 1,
                now: 0,
                token: &token,
            },
            Api::ReadFile,
            &[server, 0x1400, 3, 0, 0x1100],
        )
        .unwrap();
    files
        .execute_async(
            &mut memory,
            Some(&mut sync),
            1,
            0,
            Api::CancelIoEx,
            &[server, 0x1100],
        )
        .unwrap();
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(
        returned(
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
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 1,
                now: 0,
                token: &token,
            },
            Api::PeekNamedPipe,
            &[server, 0x1400, 3, 0x1310, 0, 0],
        )
        .unwrap();
    assert_eq!(support::dword(&memory, 0x1310), 3);
}
#[test]
fn anonymous_failed_publication_is_neutral_and_blocked_read_moves_with_its_thread() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let token = Token32::virtual_process();
    assert_eq!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: &token
                },
                Api::CreatePipe,
                &[0x1100, 0x8000, 0, 32]
            )
            .err(),
        Some(FileError32::NoAccess)
    );
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 1,
                now: 0,
                token: &token,
            },
            Api::CreatePipe,
            &[0x1100, 0x1104, 0, 32],
        )
        .unwrap();
    let reader = support::dword(&memory, 0x1100);
    let writer = support::dword(&memory, 0x1104);
    assert_eq!(reader, 0x80000000);
    assert_eq!(writer, 0x80000004);
    assert!(matches!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: &token
                },
                Api::ReadFile,
                &[reader, 0x1400, 4, 0x1300, 0]
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    let saved = files.take_thread_reply();
    memory.write(GuestAddress(0x1200), b"data").unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 2,
                        now: 0,
                        token: &token
                    },
                    Api::WriteFile,
                    &[writer, 0x1200, 4, 0x1304, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    files.restore_thread_reply(saved);
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ReadFile,
                    &[reader, 0x1400, 4, 0x1300, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::read::<4>(&memory, 0x1400), *b"data");
    assert_eq!(support::dword(&memory, 0x1300), 4);
    files
        .close_kernel_handle(&mut memory, Some(&mut sync), writer)
        .unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ReadFile,
                    &[reader, 0x1400, 4, 0x1300, 0]
                )
                .unwrap()
        ),
        (0, Some(109))
    );
}
#[test]
fn name_wait_keeps_original_deadline_and_does_not_consume_a_free_instance() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let token = Token32::virtual_process();
    support::path(&mut memory, 0x1000, r"\\.\pipe\wait");
    let name: Vec<u16> = r"\\.\pipe\wait".encode_utf16().collect();
    let server = returned(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: &token,
                },
                Api::CreateNamedPipeW,
                &[0x1000, 3, 0, 1, 16, 16, 7, 0],
            )
            .unwrap(),
    )
    .0;
    let client = returned(
        files
            .open_pipe_client(&memory, &name, &[0x1000, 0xc0000000, 0, 0, 3, 0, 0], &token)
            .unwrap(),
    )
    .0;
    assert!(matches!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 10,
                    token: &token
                },
                Api::WaitNamedPipeW,
                &[0x1000, 0]
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    assert!(matches!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 6_000_010,
                    token: &token
                },
                Api::WaitNamedPipeW,
                &[0x1000, 0]
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 7_000_010,
                        token: &token
                    },
                    Api::WaitNamedPipeW,
                    &[0x1000, 0]
                )
                .unwrap()
        ),
        (0, Some(121))
    );
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 2,
                now: 0,
                token: &token,
            },
            Api::DisconnectNamedPipe,
            &[server],
        )
        .unwrap();
    // a disconnected instance is not available until its server listens again.
    assert!(!files.pipes.available(&name).unwrap());
    files
        .pipes
        .listen(files.pipe_handle(server, 0).unwrap())
        .unwrap();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 8_000_010,
                        token: &token
                    },
                    Api::WaitNamedPipeW,
                    &[0x1000, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert!(files.pipes.available(&name).unwrap());
    files
        .close_kernel_handle(&mut memory, Some(&mut sync), client)
        .unwrap();
}

#[test]
fn call_named_pipe_preflights_before_connecting_and_releases_hidden_client_after_reply() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let token = Token32::virtual_process();
    let name: Vec<u16> = r"\\.\pipe\transaction".encode_utf16().collect();
    support::path(&mut memory, 0x1000, r"\\.\pipe\transaction");
    let server = returned(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 1,
                    now: 0,
                    token: &token,
                },
                Api::CreateNamedPipeW,
                &[0x1000, 3, 6, 1, 16, 16, 0, 0],
            )
            .unwrap(),
    )
    .0;
    memory.write(GuestAddress(0x1200), b"request").unwrap();
    assert_eq!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 2,
                    now: 0,
                    token: &token
                },
                Api::CallNamedPipeW,
                &[0x1000, 0x1200, 7, 0x8000, 5, 0x1300, 50]
            )
            .err(),
        Some(FileError32::NoAccess)
    );
    assert!(files.pipes.available(&name).unwrap());
    assert!(matches!(
        files
            .execute_pipe_api(
                &mut memory,
                Some(&mut sync),
                crate::process::windows::files::pipes::PipeCallContext32 {
                    thread: 2,
                    now: 0,
                    token: &token
                },
                Api::CallNamedPipeW,
                &[0x1000, 0x1200, 7, 0x1400, 5, 0x1300, 50]
            )
            .unwrap(),
        FileCallOutcome32::Pending
    ));
    let saved = files.take_thread_reply();
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ReadFile,
                    &[server, 0x1500, 7, 0x1304, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::read::<7>(&memory, 0x1500), *b"request");
    memory.write(GuestAddress(0x1200), b"reply").unwrap();
    files
        .execute_pipe_api(
            &mut memory,
            Some(&mut sync),
            crate::process::windows::files::pipes::PipeCallContext32 {
                thread: 1,
                now: 0,
                token: &token,
            },
            Api::WriteFile,
            &[server, 0x1200, 5, 0x1304, 0],
        )
        .unwrap();
    files.restore_thread_reply(saved);
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 2,
                        now: 0,
                        token: &token
                    },
                    Api::CallNamedPipeW,
                    &[0x1000, 0x1200, 7, 0x1400, 5, 0x1300, 50]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::read::<5>(&memory, 0x1400), *b"reply");
    assert_eq!(support::dword(&memory, 0x1300), 5);
    assert_eq!(
        returned(
            files
                .execute_pipe_api(
                    &mut memory,
                    Some(&mut sync),
                    crate::process::windows::files::pipes::PipeCallContext32 {
                        thread: 1,
                        now: 0,
                        token: &token
                    },
                    Api::ReadFile,
                    &[server, 0x1500, 1, 0x1304, 0]
                )
                .unwrap()
        ),
        (0, Some(109))
    );
}
