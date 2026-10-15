use super::returned;
use crate::{
    files::{FileAccess32, FileError32},
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::files::tests::support::*,
    windows::files::FileApi32 as Api,
};

#[test]
fn allocation_reservations_and_explicit_base_alignment_are_enforced_before_publication() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 4, 0, 4096, 0],
    ));
    let reserved = [PageRange::new(GuestAddress(0x1000_0000), 16).unwrap()];
    let view = returned(files.execute_mapping(
        &mut memory,
        &reserved,
        Api::MapViewOfFile,
        &[section, 4, 0, 0, 0],
    ));
    assert_eq!(view, 0x1001_0000);
    let before = memory.transaction_version();
    assert!(matches!(
        files.execute_mapping(
            &mut memory,
            &reserved,
            Api::MapViewOfFileEx,
            &[section, 4, 0, 0, 0, 0x1000_1000]
        ),
        Err(FileError32::MappedAlignment)
    ));
    assert_eq!(memory.transaction_version(), before);
    assert!(matches!(
        files.execute_mapping(
            &mut memory,
            &reserved,
            Api::MapViewOfFileEx,
            &[section, 4, 0, 0, 0, 0x1000_0000]
        ),
        Err(FileError32::InvalidAddress)
    ));
}

#[test]
fn reserve_section_commits_shared_pages_without_replacing_backing_or_exceeding_rights() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 0x0400_0004, 0, 4096, 0],
    ));
    let a = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 2, 0, 0, 0],
    ));
    let b = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 4, 0, 0, 0],
    ));
    assert!(memory.read(GuestAddress(a), &mut [0]).is_err());
    assert!(
        files
            .try_commit_reserved_view(
                &mut memory,
                PageRange::new(GuestAddress(a), 1).unwrap(),
                Permissions::READ_WRITE
            )
            .unwrap()
    );
    memory.write(GuestAddress(a), &[7]).unwrap();
    assert_eq!(
        read::<1>(&memory, b),
        [7],
        "commitment reaches other views with their own rights"
    );
    let future = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 4, 0, 0, 0],
    ));
    assert_eq!(
        read::<1>(&memory, future),
        [7],
        "future views observe prior section commitment"
    );
    assert!(
        files
            .try_commit_reserved_view(
                &mut memory,
                PageRange::new(GuestAddress(b), 1).unwrap(),
                Permissions::READ
            )
            .unwrap()
    );
    assert_eq!(read::<1>(&memory, b), [7]);
    assert!(matches!(
        files.try_commit_reserved_view(
            &mut memory,
            PageRange::new(GuestAddress(b), 1).unwrap(),
            Permissions::READ_WRITE
        ),
        Err(FileError32::AccessDenied)
    ));
}

#[test]
fn narrowed_file_handle_cannot_create_a_writable_or_executable_section() {
    let (mut files, mut memory, _) = runtime();
    let file = open(&mut files, "C:\\rights.bin", FileAccess32::ALL);
    write_file(&mut files, file, &[1]);
    let open = files.handles.file(file).unwrap();
    files.vfs.retain_open(open).unwrap();
    let prepared = files.handles.prepare(FileAccess32::READ.0, false).unwrap();
    let narrow = files.handles.commit(
        prepared,
        crate::process::windows::files::handles::KernelFileObject32::File(open),
    );
    assert!(matches!(
        files.execute_mapping(
            &mut memory,
            &[],
            Api::CreateFileMappingW,
            &[narrow, 0, 4, 0, 0, 0]
        ),
        Err(FileError32::AccessDenied)
    ));
    assert!(matches!(
        files.execute_mapping(
            &mut memory,
            &[],
            Api::CreateFileMappingW,
            &[narrow, 0, 0x20, 0, 0, 0]
        ),
        Err(FileError32::AccessDenied)
    ));
}

#[test]
fn view_protection_never_escalates_original_view_access_or_uncommitted_reservations() {
    let (mut files, mut memory, _) = runtime();
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[u32::MAX, 0, 0x0400_0004, 0, 4096, 0],
    ));
    let view = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 4, 0, 0, 0],
    ));
    let range = PageRange::new(GuestAddress(view), 1).unwrap();
    assert!(matches!(
        files
            .mappings
            .validate_view_protection(range, Permissions::READ),
        Err(FileError32::InvalidAddress)
    ));
    files
        .try_commit_reserved_view(&mut memory, range, Permissions::READ)
        .unwrap();
    assert!(matches!(
        files
            .mappings
            .validate_view_protection(range, Permissions::READ_WRITE),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(
        files
            .mappings
            .validate_view_protection(range, Permissions::NONE),
        Ok(true)
    );
}
