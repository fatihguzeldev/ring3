use super::super::{FileCallOutcome32, tests::support};
use crate::{
    memory::GuestAddress,
    process::windows::threads::ThreadRuntime32,
    windows::{
        files::{FileApi32 as Api, devices::DeviceName32},
        thread::ThreadLimits32,
    },
};
fn returned(outcome: FileCallOutcome32) -> (u32, Option<u32>) {
    match outcome {
        FileCallOutcome32::Return { value, last_error } => (value, last_error),
        _ => panic!("null device completes immediately"),
    }
}
#[test]
fn null_device_discards_without_overlay_and_duplicates_preserve_iocp_and_event_authority() {
    let (mut files, mut memory, mut sync) = support::runtime();
    let threads = ThreadRuntime32::new(ThreadLimits32::default()).unwrap();
    let handle = returned(
        files
            .open_device(
                &memory,
                DeviceName32::Null,
                &[0, 0xc0000000, 0, 0, 3, 0x40000000, 0],
                false,
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
                &[handle, 0, 0x9876, 1],
            )
            .unwrap(),
    )
    .0;
    let (event, _) = sync.create_event(true, false, false, None).unwrap();
    let mut overlap = [0; 20];
    overlap[16..].copy_from_slice(&event.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    memory.write(GuestAddress(0x1200), b"discard").unwrap();
    assert_eq!(
        returned(
            files
                .transfer_null_device(
                    &mut memory,
                    Some(&mut sync),
                    Api::WriteFile,
                    &[handle, 0x1200, 7, 0x1300, 0x1100]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::dword(&memory, 0x1300), 7);
    assert!(sync.event_is_signaled(event).unwrap());
    assert_eq!(
        returned(
            files
                .execute_completion_api(
                    &mut memory,
                    &threads,
                    0,
                    1,
                    Api::GetQueuedCompletionStatus,
                    &[port, 0x1400, 0x1404, 0x1408, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::dword(&memory, 0x1400), 7);
    assert_eq!(support::dword(&memory, 0x1404), 0x9876);
    assert!(files.vfs.overlay_entry(0).unwrap().is_none());
    assert_eq!(
        returned(
            files
                .transfer_null_device(
                    &mut memory,
                    Some(&mut sync),
                    Api::ReadFile,
                    &[handle, 0x1200, 7, 0x1300, 0x1100]
                )
                .unwrap()
        ),
        (0, Some(38))
    );
    assert_eq!(support::read::<7>(&memory, 0x1200), *b"discard");
    assert_eq!(
        returned(
            files
                .execute_async(
                    &mut memory,
                    Some(&mut sync),
                    1,
                    0,
                    Api::GetOverlappedResult,
                    &[handle, 0x1100, 0x1300, 0]
                )
                .unwrap()
        ),
        (0, Some(38))
    );
    assert_eq!(
        returned(
            files
                .execute_async(
                    &mut memory,
                    Some(&mut sync),
                    1,
                    0,
                    Api::CancelIoEx,
                    &[handle, 0]
                )
                .unwrap()
        ),
        (0, Some(1168))
    );
    files
        .duplicate_kernel_handle(
            &mut memory,
            Some(&mut sync),
            &[u32::MAX, handle, u32::MAX, 0x1500, 0, 0, 2],
            None,
        )
        .unwrap();
    let duplicate = support::dword(&memory, 0x1500);
    files
        .close_kernel_handle(&mut memory, Some(&mut sync), handle)
        .unwrap();
    assert_eq!(
        returned(
            files
                .null_metadata(&mut memory, Api::GetFileType, &[duplicate])
                .unwrap()
        ),
        (2, None)
    );
    files
        .close_kernel_handle(&mut memory, Some(&mut sync), duplicate)
        .unwrap();
    assert!(
        files
            .null_metadata(&mut memory, Api::GetFileType, &[duplicate])
            .is_err()
    );
}
#[test]
fn synchronous_null_read_is_eof_and_console_names_do_not_create_files_in_headless_profile() {
    let (mut files, mut memory, _) = support::runtime();
    for name in [
        DeviceName32::Console,
        DeviceName32::ConsoleInput,
        DeviceName32::ConsoleOutput,
    ] {
        assert_eq!(
            files
                .open_device(&memory, name, &[0, 0xc0000000, 0, 0, 3, 0, 0], false)
                .err(),
            Some(crate::files::FileError32::NotFound)
        );
    }
    let handle = returned(
        files
            .open_device(
                &memory,
                DeviceName32::Null,
                &[0, 0xc0000000, 0, 0, 2, 0, 0],
                false,
            )
            .unwrap(),
    )
    .0;
    memory.write(GuestAddress(0x1200), b"keep").unwrap();
    assert_eq!(
        returned(
            files
                .transfer_null_device(
                    &mut memory,
                    None,
                    Api::ReadFile,
                    &[handle, 0x1200, 4, 0x1300, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::dword(&memory, 0x1300), 0);
    assert_eq!(support::read::<4>(&memory, 0x1200), *b"keep");
    assert_eq!(
        returned(
            files
                .null_metadata(&mut memory, Api::GetFileSizeEx, &[handle, 0x1300])
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::read::<8>(&memory, 0x1300), [0; 8]);
}
