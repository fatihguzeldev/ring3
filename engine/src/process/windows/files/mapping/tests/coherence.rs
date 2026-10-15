use super::returned;
use crate::{
    files::FileAccess32,
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::files::tests::support::*,
    windows::files::FileApi32 as Api,
};

#[test]
fn overlapping_views_from_distinct_mapping_objects_share_bytes_and_code_versions() {
    let (mut files, mut memory, _) = runtime();
    let file = open(&mut files, "C:\\coherent.bin", FileAccess32::ALL);
    write_file(&mut files, file, &vec![0x90; 8192]);
    let first = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[file, 0, 0x40, 0, 0, 0],
    ));
    let second = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[file, 0, 0x40, 0, 0, 0],
    ));
    let writer = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[first, 2, 0, 0, 8192],
    ));
    let code = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[second, 0x24, 0, 0, 4096],
    ));
    let snapshot = memory.snapshot_code(GuestAddress(code), 1).unwrap();
    memory.write(GuestAddress(writer), &[0xc3]).unwrap();
    assert_eq!(read::<1>(&memory, code), [0xc3]);
    assert!(!memory.is_code_current(&snapshot));
    assert!(memory.write(GuestAddress(code), &[0]).is_err());
    memory
        .protect(
            PageRange::new(GuestAddress(writer), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert_eq!(
        memory.permissions_at(GuestAddress(code)),
        Ok(Permissions::READ_EXECUTE)
    );
}

#[test]
fn copy_view_is_coherent_until_first_write_and_never_flushes_private_bytes() {
    let (mut files, mut memory, _) = runtime();
    let file = open(&mut files, "C:\\copy.bin", FileAccess32::ALL);
    write_file(&mut files, file, &[1, 2, 3]);
    let section = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::CreateFileMappingW,
        &[file, 0, 4, 0, 0, 0],
    ));
    let shared = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 2, 0, 0, 0],
    ));
    let private = returned(files.execute_mapping(
        &mut memory,
        &[],
        Api::MapViewOfFile,
        &[section, 1, 0, 0, 0],
    ));
    memory.write(GuestAddress(shared), &[7]).unwrap();
    assert_eq!(read::<3>(&memory, private), [7, 2, 3]);
    memory.write(GuestAddress(private + 1), &[8]).unwrap();
    memory.write(GuestAddress(shared), &[9]).unwrap();
    assert_eq!(read::<3>(&memory, private), [7, 8, 3]);
    assert_eq!(
        returned(files.execute_mapping(&mut memory, &[], Api::FlushViewOfFile, &[private, 0])),
        1
    );
    assert_eq!(file_bytes(&mut files, file), [1, 2, 3]);
    assert_eq!(
        returned(files.execute_mapping(&mut memory, &[], Api::FlushViewOfFile, &[shared, 0])),
        1
    );
    assert_eq!(file_bytes(&mut files, file), [9, 2, 3]);
}
