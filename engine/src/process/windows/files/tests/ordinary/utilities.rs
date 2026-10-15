use super::{returned, support};
use crate::{
    files::{FileAccess32, FileError32},
    memory::GuestAddress,
    windows::files::FileApi32,
};

#[test]
fn temporary_unique_zero_creates_file_but_explicit_unique_only_returns_name() {
    let (mut runtime, mut memory, _) = support::runtime();
    support::path(&mut memory, 0x1000, "C:\\");
    support::path(&mut memory, 0x1100, "prefix");
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetTempFileNameW,
                    &[0x1000, 0x1100, 0x12345678, 0x1800]
                )
                .unwrap()
        ),
        (0x12345678, None)
    );
    assert_eq!(
        support::read::<30>(&memory, 0x1800),
        [
            67, 0, 58, 0, 92, 0, 112, 0, 114, 0, 101, 0, 53, 0, 54, 0, 55, 0, 56, 0, 46, 0, 84, 0,
            77, 0, 80, 0, 0, 0
        ]
    );
    assert_eq!(
        runtime
            .vfs
            .path_info(&"C:\\pre5678.TMP".encode_utf16().collect::<Vec<_>>()),
        Err(FileError32::NotFound)
    );
    let (unique, _) = returned(
        runtime
            .execute_ordinary(
                &mut memory,
                FileApi32::GetTempFileNameW,
                &[0x1000, 0x1100, 0, 0x1800],
            )
            .unwrap(),
    );
    assert_ne!(unique, 0);
    assert_eq!(
        runtime
            .vfs
            .path_info(&"C:\\pre0001.TMP".encode_utf16().collect::<Vec<_>>())
            .unwrap()
            .size,
        0
    );
}
#[test]
fn search_fullpath_and_finalpath_publish_correct_part_and_opened_identity() {
    let (mut runtime, mut memory, _) = support::runtime();
    let handle = support::open(&mut runtime, "C:\\asset.bin", FileAccess32::ALL);
    support::path(&mut memory, 0x1000, "asset");
    support::path(&mut memory, 0x1100, ".bin");
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::SearchPathW,
                    &[0, 0x1000, 0x1100, 64, 0x1800, 0x1a00]
                )
                .unwrap()
        ),
        (12, None)
    );
    assert_eq!(support::dword(&memory, 0x1a00), 0x1806);
    let rename = runtime
        .vfs
        .prepare_move(
            &"C:\\asset.bin".encode_utf16().collect::<Vec<_>>(),
            &"C:\\renamed.bin".encode_utf16().collect::<Vec<_>>(),
            false,
        )
        .unwrap();
    runtime.vfs.commit_namespace(rename);
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetFinalPathNameByHandleW,
                    &[handle, 0x1800, 64, 8]
                )
                .unwrap()
        ),
        (16, None)
    );
    assert_eq!(
        &support::read::<32>(&memory, 0x1800)[..14],
        &[92, 0, 92, 0, 63, 0, 92, 0, 67, 0, 58, 0, 92, 0]
    );
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetFinalPathNameByHandleW,
                    &[handle, 0x1800, 64, 4]
                )
                .unwrap()
        ),
        (12, None)
    );
    support::path(&mut memory, 0x1000, "C:\\Windows\\");
    runtime
        .execute_ordinary(
            &mut memory,
            FileApi32::GetFullPathNameW,
            &[0x1000, 64, 0x1800, 0x1a00],
        )
        .unwrap();
    assert_eq!(support::dword(&memory, 0x1a00), 0);
}
#[test]
fn extended_standard_and_name_layout_and_small_output_are_literal() {
    let (mut runtime, mut memory, _) = support::runtime();
    let handle = support::open(&mut runtime, "C:\\a.bin", FileAccess32::ALL);
    support::write_file(&mut runtime, handle, b"123");
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetFileInformationByHandleEx,
                    &[handle, 1, 0x1800, 24]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(
        support::read::<24>(&memory, 0x1800),
        [
            0, 0, 1, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    memory.write(GuestAddress(0x1800), &[0x77; 24]).unwrap();
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetFileInformationByHandleEx,
                    &[handle, 0, 0x1800, 24]
                )
                .unwrap()
        ),
        (0, Some(24))
    );
    assert_eq!(support::read::<24>(&memory, 0x1800), [0x77; 24]);
    runtime
        .execute_ordinary(
            &mut memory,
            FileApi32::GetFileInformationByHandleEx,
            &[handle, 2, 0x1800, 64],
        )
        .unwrap();
    assert_eq!(
        support::read::<16>(&memory, 0x1800),
        [12, 0, 0, 0, 92, 0, 97, 0, 46, 0, 98, 0, 105, 0, 110, 0]
    );
}

#[test]
fn directory_information_uses_aligned_records_and_failed_output_preserves_cursor() {
    use crate::files::{FileDisposition32, FileOpenOptions32};
    let (mut runtime, mut memory, _) = support::runtime();
    support::open(&mut runtime, "C:\\a.bin", FileAccess32::ALL);
    support::open(&mut runtime, "C:\\b.bin", FileAccess32::ALL);
    let directory = support::open_options(
        &mut runtime,
        "C:\\",
        FileOpenOptions32 {
            access: FileAccess32::READ,
            directory: true,
            disposition: FileDisposition32::OpenExisting,
            ..FileOpenOptions32::default()
        },
    );
    assert_eq!(
        runtime.execute_ordinary(
            &mut memory,
            FileApi32::GetFileInformationByHandleEx,
            &[directory, 15, 0x6000, 80]
        ),
        Err(FileError32::NoAccess)
    );
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    FileApi32::GetFileInformationByHandleEx,
                    &[directory, 14, 0x1800, 80]
                )
                .unwrap()
        ),
        (1, None)
    );
    assert_eq!(support::dword(&memory, 0x1800), 0);
    assert_eq!(support::dword(&memory, 0x183c), 10);
    assert_eq!(
        support::read::<10>(&memory, 0x1844),
        [97, 0, 46, 0, 98, 0, 105, 0, 110, 0]
    );
    runtime
        .execute_ordinary(
            &mut memory,
            FileApi32::GetFileInformationByHandleEx,
            &[directory, 14, 0x1800, 80],
        )
        .unwrap();
    assert_eq!(
        support::read::<10>(&memory, 0x1844),
        [98, 0, 46, 0, 98, 0, 105, 0, 110, 0]
    );
    assert_eq!(
        runtime.execute_ordinary(
            &mut memory,
            FileApi32::GetFileInformationByHandleEx,
            &[directory, 14, 0x1800, 256]
        ),
        Err(FileError32::NoMoreFiles)
    );
    runtime
        .execute_ordinary(
            &mut memory,
            FileApi32::GetFileInformationByHandleEx,
            &[directory, 15, 0x1800, 256],
        )
        .unwrap();
    assert_eq!(support::dword(&memory, 0x1800), 80);
    assert_eq!(support::dword(&memory, 0x1850), 0);
}
