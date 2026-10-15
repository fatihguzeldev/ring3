use super::{returned, support, support::*};
use crate::{
    files::{FileAccess32, FileError32},
    memory::GuestAddress,
    process::windows::files::handles::KernelFileObject32,
    windows::files::FileApi32 as Api,
};

#[test]
fn invalid_read_output_preserves_cursor_and_write_count_failure_preserves_contents() {
    let (mut runtime, mut memory, _) = runtime();
    path(&mut memory, 0x1200, "C:\\asset.bin");
    let (handle, error) = returned(
        runtime
            .execute_ordinary(
                &mut memory,
                Api::CreateFileW,
                &[0x1200, 0xc000_0000, 3, 0, 2, 128, 0],
            )
            .unwrap(),
    );
    assert_eq!(error, Some(0));
    memory.write(GuestAddress(0x1800), b"sample").unwrap();
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::WriteFile, &[handle, 0x1800, 6, 0x1000, 0])
                .unwrap()
        )
        .0,
        1
    );
    assert_eq!(dword(&memory, 0x1000), 6);
    runtime
        .execute_ordinary(
            &mut memory,
            Api::SetFilePointerEx,
            &[handle, 0, 0, 0x1010, 0],
        )
        .unwrap();
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::ReadFile, &[handle, 0x6000, 8, 0x1000, 0])
                .unwrap()
        ),
        (0, Some(998))
    );
    assert_eq!(dword(&memory, 0x1000), 0);
    let open = runtime.handles.file(handle).unwrap();
    assert_eq!(runtime.vfs.position(open), Ok(0));
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::ReadFile, &[handle, 0x1800, 8, 0x1000, 0])
                .unwrap()
        )
        .0,
        1
    );
    assert_eq!(dword(&memory, 0x1000), 6);
    assert_eq!(read::<6>(&memory, 0x1800), *b"sample");
    assert_eq!(
        runtime.execute_ordinary(&mut memory, Api::WriteFile, &[handle, 0x1800, 1, 0x6000, 0]),
        Err(FileError32::NoAccess)
    );
    assert_eq!(file_bytes(&mut runtime, handle), b"sample");
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::ReadFile, &[handle, 0x1800, 1, 0x1000, 0])
                .unwrap()
        )
        .0,
        1
    );
    assert_eq!(dword(&memory, 0x1000), 0);
}

#[test]
fn large_file_positions_preserve_full_width_and_old_seek_failure_is_neutral() {
    let (mut runtime, mut memory, _) = runtime();
    let handle = open(&mut runtime, "C:\\large.bin", FileAccess32::ALL);
    runtime
        .execute_ordinary(
            &mut memory,
            Api::SetFilePointerEx,
            &[handle, 5, 1, 0x1010, 0],
        )
        .unwrap();
    assert_eq!(
        u64::from_le_bytes(read::<8>(&memory, 0x1010)),
        0x1_0000_0005
    );
    runtime
        .execute_ordinary(&mut memory, Api::SetEndOfFile, &[handle])
        .unwrap();
    runtime
        .execute_ordinary(&mut memory, Api::GetFileSizeEx, &[handle, 0x1020])
        .unwrap();
    assert_eq!(
        u64::from_le_bytes(read::<8>(&memory, 0x1020)),
        0x1_0000_0005
    );
    runtime
        .execute_ordinary(&mut memory, Api::GetFileSize, &[handle, 0x1000])
        .unwrap();
    assert_eq!(dword(&memory, 0x1000), 1);
    assert_eq!(
        runtime.execute_ordinary(&mut memory, Api::SetFilePointer, &[handle, 0, 0, 2]),
        Err(FileError32::InvalidParameter)
    );
    assert_eq!(
        runtime.vfs.position(runtime.handles.file(handle).unwrap()),
        Ok(0x1_0000_0005)
    );
}

#[test]
fn narrowed_duplicate_append_access_uses_eof_and_cannot_read_original_rw_description() {
    let (mut runtime, mut memory, _) = runtime();
    let original = open(&mut runtime, "C:\\append.bin", FileAccess32::ALL);
    write_file(&mut runtime, original, b"abc");
    let open = runtime.handles.file(original).unwrap();
    runtime.vfs.retain_open(open).unwrap();
    let prepared = runtime
        .handles
        .prepare(FileAccess32::APPEND.0, false)
        .unwrap();
    let append = runtime
        .handles
        .commit(prepared, KernelFileObject32::File(open));
    memory.write(GuestAddress(0x1800), b"x").unwrap();
    runtime
        .execute_ordinary(&mut memory, Api::WriteFile, &[append, 0x1800, 1, 0x1000, 0])
        .unwrap();
    assert_eq!(file_bytes(&mut runtime, original), b"abcx");
    assert_eq!(runtime.vfs.position(open), Ok(4));
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::ReadFile, &[append, 0x1800, 1, 0x1000, 0])
                .unwrap()
        ),
        (0, Some(5))
    );
}

#[test]
fn handle_capacity_failure_precedes_destructive_create_always() {
    let (mut runtime, mut memory, _) = runtime();
    let first = open(&mut runtime, "C:\\retained.bin", FileAccess32::ALL);
    write_file(&mut runtime, first, b"retained");
    for index in 0..63 {
        open(&mut runtime, &format!("C:\\f{index}"), FileAccess32::ALL);
    }
    path(&mut memory, 0x1200, "C:\\retained.bin");
    assert_eq!(
        runtime.execute_ordinary(
            &mut memory,
            Api::CreateFileW,
            &[0x1200, 0xc000_0000, 7, 0, 2, 128, 0]
        ),
        Err(FileError32::Capacity)
    );
    assert_eq!(file_bytes(&mut runtime, first), b"retained");
}

#[test]
fn huge_invalid_write_fails_before_host_allocation_and_preserves_file() {
    let (mut runtime, mut memory, _) = support::runtime();
    let handle = support::open(&mut runtime, "C:\\guard.bin", FileAccess32::ALL);
    support::write_file(&mut runtime, handle, b"unchanged");
    memory
        .write(GuestAddress(0x1800), &0xfeedu32.to_le_bytes())
        .unwrap();
    let open = runtime.handles.file(handle).unwrap();
    let revision = runtime.vfs.revision();
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    Api::WriteFile,
                    &[handle, 0x1000, u32::MAX, 0x1800, 0]
                )
                .unwrap()
        ),
        (0, Some(998))
    );
    assert_eq!(support::dword(&memory, 0x1800), 0);
    assert_eq!(runtime.vfs.revision(), revision);
    assert_eq!(runtime.vfs.position(open).unwrap(), 0);
    assert_eq!(support::file_bytes(&mut runtime, handle), b"unchanged");
}
