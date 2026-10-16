use super::{returned, support};
mod termination;
use crate::{
    files::{FileAccess32, FileError32},
    memory::{GuestAddress, PageRange, Permissions},
    process::{
        WindowsCallback32, WindowsCallbackProvider32, windows::files::ordinary::FileCallOutcome32,
    },
    windows::files::FileApi32,
};

fn callback(outcome: FileCallOutcome32) -> WindowsCallback32 {
    match outcome {
        FileCallOutcome32::Callback(callback) => callback,
        _ => panic!("expected copy callback"),
    }
}
fn setup() -> (
    crate::process::windows::files::FileRuntime32,
    crate::memory::AddressSpace,
    u32,
) {
    let (mut runtime, mut memory, _) = support::runtime();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x8000), 1).unwrap(),
            Permissions::READ_EXECUTE,
        )
        .unwrap();
    let source = support::open(&mut runtime, "C:\\source.bin", FileAccess32::ALL);
    support::write_file(&mut runtime, source, b"source bytes");
    support::path(&mut memory, 0x1000, "C:\\source.bin");
    support::path(&mut memory, 0x1100, "C:\\target.bin");
    (runtime, memory, source)
}
#[test]
fn progress_arguments_and_quiet_preserve_lazy_copy_with_real_handles() {
    let (mut runtime, mut memory, source) = setup();
    let first = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0xfeed, 0, 0],
            )
            .unwrap(),
    );
    assert_eq!(first.provider, WindowsCallbackProvider32::Files);
    assert_eq!(first.count, 13);
    assert_eq!(&first.arguments[..10], &[12, 0, 0, 0, 12, 0, 0, 0, 1, 1]);
    assert_eq!(first.arguments[12], 0xfeed);
    assert!(runtime.handles.file(first.arguments[10]).is_ok());
    assert!(runtime.handles.file(first.arguments[11]).is_ok());
    assert_eq!(support::file_bytes(&mut runtime, first.arguments[11]), b"");
    let revision = runtime.vfs.revision();
    assert_eq!(
        runtime.complete_file_callback(&mut memory, first.continuation + 1, 0),
        Err(FileError32::InvalidHandle)
    );
    assert_eq!(runtime.vfs.revision(), revision);
    assert_eq!(
        returned(
            runtime
                .complete_file_callback(&mut memory, first.continuation, 3)
                .unwrap()
        ),
        (1, None)
    );
    assert!(runtime.handles.file(first.arguments[10]).is_err());
    assert!(runtime.handles.file(first.arguments[11]).is_err());
    let target = support::open(&mut runtime, "C:\\target.bin", FileAccess32::ALL);
    assert_eq!(
        support::file_bytes(&mut runtime, target),
        support::file_bytes(&mut runtime, source)
    );
    assert_eq!(
        runtime.complete_file_callback(&mut memory, first.continuation, 0),
        Err(FileError32::InvalidHandle)
    );
}
#[test]
fn cancel_after_final_callback_deletes_only_held_destination_after_name_replacement() {
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
    let last = callback(
        runtime
            .complete_file_callback(&mut memory, first.continuation, 0)
            .unwrap(),
    );
    assert_eq!(&last.arguments[..10], &[12, 0, 12, 0, 12, 0, 12, 0, 1, 0]);
    let move_target = runtime
        .vfs
        .prepare_move(
            &"C:\\target.bin".encode_utf16().collect::<Vec<_>>(),
            &"C:\\held.bin".encode_utf16().collect::<Vec<_>>(),
            false,
        )
        .unwrap();
    runtime.vfs.commit_namespace(move_target);
    let replacement = support::open(&mut runtime, "C:\\target.bin", FileAccess32::ALL);
    support::write_file(&mut runtime, replacement, b"replacement");
    assert_eq!(
        returned(
            runtime
                .complete_file_callback(&mut memory, last.continuation, 1)
                .unwrap()
        ),
        (0, Some(1235))
    );
    assert_eq!(
        support::file_bytes(&mut runtime, replacement),
        b"replacement"
    );
    assert_eq!(
        runtime
            .vfs
            .path_info(&"C:\\held.bin".encode_utf16().collect::<Vec<_>>()),
        Err(FileError32::NotFound)
    );
}
#[test]
fn stop_preserves_empty_destination_and_same_identity_never_truncates_source() {
    let (mut runtime, mut memory, source) = setup();
    let first = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0, 0, 0],
            )
            .unwrap(),
    );
    assert_eq!(
        returned(
            runtime
                .complete_file_callback(&mut memory, first.continuation, 2)
                .unwrap()
        ),
        (0, Some(1235))
    );
    let target = support::open(&mut runtime, "C:\\target.bin", FileAccess32::ALL);
    assert_eq!(support::file_bytes(&mut runtime, target), b"");
    assert_eq!(
        runtime.execute_ordinary(
            &mut memory,
            FileApi32::CopyFileExW,
            &[0x1000, 0x1000, 0x8000, 0, 0, 0]
        ),
        Err(FileError32::SharingViolation)
    );
    assert_eq!(support::file_bytes(&mut runtime, source), b"source bytes");
}

#[test]
fn literal_copy_flags_require_writable_source_and_cancel_flag_is_rechecked() {
    let (mut runtime, mut memory, source) = setup();
    let readonly = runtime
        .vfs
        .prepare_set_attributes(&"C:\\source.bin".encode_utf16().collect::<Vec<_>>(), 1)
        .unwrap();
    runtime.vfs.commit_namespace(readonly);
    assert_eq!(
        runtime.execute_ordinary(
            &mut memory,
            FileApi32::CopyFileExW,
            &[0x1000, 0x1100, 0x8000, 0, 0, 4]
        ),
        Err(FileError32::AccessDenied)
    );
    assert_eq!(support::file_bytes(&mut runtime, source), b"source bytes");
    let first = callback(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::CopyFileExW,
                &[0x1000, 0x1100, 0x8000, 0, 0x1400, 0x1808],
            )
            .unwrap(),
    );
    memory
        .write(GuestAddress(0x1400), &1u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        returned(
            runtime
                .complete_file_callback(&mut memory, first.continuation, 0)
                .unwrap()
        ),
        (0, Some(1235))
    );
    assert_eq!(
        runtime
            .vfs
            .path_info(&"C:\\target.bin".encode_utf16().collect::<Vec<_>>()),
        Err(FileError32::NotFound)
    );
}
