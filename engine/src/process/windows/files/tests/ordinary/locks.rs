use super::support::*;
use crate::files::{FileAccess32, FileError32};

#[test]
fn same_description_shared_over_exclusive_requires_exclusive_unlock_first() {
    let (mut runtime, _, _) = runtime();
    let first = open(&mut runtime, "C:\\locked.bin", FileAccess32::ALL);
    let second = open(&mut runtime, "C:\\locked.bin", FileAccess32::ALL);
    let first = runtime.handles.file(first).unwrap();
    let second = runtime.handles.file(second).unwrap();
    let exclusive = runtime.prepare_file_lock(first, 3, 5, true).unwrap();
    runtime.commit_file_lock(exclusive);
    assert_eq!(runtime.check_file_lock(first, 3, 5, true), Ok(()));
    assert_eq!(
        runtime.check_file_lock(second, 3, 5, false),
        Err(FileError32::LockViolation)
    );
    let shared = runtime.prepare_file_lock(first, 3, 5, false).unwrap();
    runtime.commit_file_lock(shared);
    assert_eq!(
        runtime.check_file_lock(first, 3, 5, true),
        Err(FileError32::LockViolation)
    );
    assert_eq!(
        runtime.check_unlock_file_range(first, 3, 4),
        Err(FileError32::LockViolation)
    );
    runtime.unlock_file_range(first, 3, 5).unwrap();
    assert_eq!(runtime.check_file_lock(second, 3, 5, false), Ok(()));
    assert_eq!(
        runtime.check_file_lock(second, 3, 5, true),
        Err(FileError32::LockViolation)
    );
    runtime.unlock_file_range(first, 3, 5).unwrap();
    assert_eq!(runtime.check_file_lock(second, 3, 5, true), Ok(()));
}
