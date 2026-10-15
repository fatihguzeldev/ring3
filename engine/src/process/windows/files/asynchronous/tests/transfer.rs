use super::{result, setup};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::files::{asynchronous::AsyncProgress32, tests::support::*},
    windows::files::FileApi32 as Api,
};

#[test]
fn accepted_read_completes_once_signals_events_and_preserves_file_cursor() {
    let (mut files, mut memory, mut sync, file, event, file_event) = setup();
    let open = files.handles.file(file).unwrap();
    memory.write(GuestAddress(0x1200), &[9; 8]).unwrap();
    assert_eq!(
        result(
            files
                .execute_overlapped(
                    &mut memory,
                    &mut sync,
                    1,
                    file_event,
                    Api::ReadFile,
                    &[file, 0x1200, 8, 0x1300, 0x1100]
                )
                .unwrap()
        ),
        (0, Some(997))
    );
    assert!(!sync.event_is_signaled(event).unwrap());
    assert_eq!(dword(&memory, 0x1100), 0x103);
    assert_eq!(read::<3>(&memory, 0x1200), [9; 3]);
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Completed {
            bytes: 3,
            error: 0,
            ..
        }
    ));
    assert_eq!(read::<5>(&memory, 0x1200), [1, 2, 3, 9, 9]);
    assert!(sync.event_is_signaled(event).unwrap());
    assert!(sync.event_is_signaled(file_event).unwrap());
    assert_eq!(files.vfs.position(open).unwrap(), 0);
    assert_eq!(
        result(
            files
                .execute_async(
                    &mut memory,
                    Some(&mut sync),
                    1,
                    0,
                    Api::GetOverlappedResult,
                    &[file, 0x1100, 0x1300, 0]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(dword(&memory, 0x1300), 3);
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Idle
    ));
}

#[test]
fn write_snapshots_input_and_completion_failure_does_not_modify_file_or_signal() {
    let (mut files, mut memory, mut sync, file, event, file_event) = setup();
    memory.write(GuestAddress(0x1200), &[4, 5]).unwrap();
    files
        .execute_overlapped(
            &mut memory,
            &mut sync,
            1,
            file_event,
            Api::WriteFile,
            &[file, 0x1200, 2, 0, 0x1100],
        )
        .unwrap();
    memory.write(GuestAddress(0x1200), &[8, 8]).unwrap();
    files.progress_io(&mut memory, Some(&mut sync)).unwrap();
    assert_eq!(file_bytes(&mut files, file), [4, 5, 3]);
    assert!(sync.event_is_signaled(event).unwrap());
    memory.write(GuestAddress(0x1200), &[7]).unwrap();
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
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert!(files.progress_io(&mut memory, Some(&mut sync)).is_err());
    assert_eq!(file_bytes(&mut files, file), [4, 5, 3]);
    assert!(!sync.event_is_signaled(event).unwrap());
}

#[test]
fn synchronous_overlapped_uses_explicit_offset_and_updates_shared_cursor() {
    let (mut files, mut memory, _, file, _, _) = setup();
    memory
        .write(GuestAddress(0x1108), &1u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        result(
            files
                .execute_positioned(
                    &mut memory,
                    Api::ReadFile,
                    &[file, 0x1200, 2, 0x1300, 0x1100]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(read::<2>(&memory, 0x1200), [2, 3]);
    let open = files.handles.file(file).unwrap();
    assert_eq!(files.vfs.position(open).unwrap(), 3);
}
