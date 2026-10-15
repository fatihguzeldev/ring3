use super::{result, setup};
use crate::{
    files::{FileAccess32, FileDisposition32, FileOpenOptions32},
    process::windows::files::{asynchronous::AsyncProgress32, tests::support::*},
    windows::files::FileApi32 as Api,
};

#[test]
fn pending_lock_waits_for_conflict_release_then_signals_and_can_be_unlocked() {
    let (mut files, mut memory, mut sync, file, event, _) = setup();
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
    let lock = files.prepare_file_lock(owner, 0, 2, true).unwrap();
    files.commit_file_lock(lock);
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
        (0, Some(997))
    );
    assert!(!sync.event_is_signaled(event).unwrap());
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Idle
    ));
    files.unlock_file_range(owner, 0, 2).unwrap();
    assert!(matches!(
        files.progress_io(&mut memory, Some(&mut sync)).unwrap(),
        AsyncProgress32::Completed {
            error: 0,
            bytes: 0,
            ..
        }
    ));
    assert!(sync.event_is_signaled(event).unwrap());
    assert_eq!(
        result(
            files
                .execute_lock_ex(
                    &mut memory,
                    Some(&mut sync),
                    1,
                    Api::UnlockFileEx,
                    &[file, 0, 2, 0, 0x1100]
                )
                .unwrap()
        ),
        (1, None)
    );
}
