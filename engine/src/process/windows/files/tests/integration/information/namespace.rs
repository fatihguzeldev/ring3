use super::*;

#[test]
fn captured_handle_rename_supports_counted_names_and_its_own_exclusive_description() {
    let mut engine = engine();
    let file = create(&mut engine, r"C:\exclusive.bin", 0xc0010000, 0, 0);
    assert_ne!(file, u32::MAX);
    let name = r"C:\renamed.bin".encode_utf16().collect::<Vec<_>>();
    let mut rename = vec![0; 12];
    rename[8..12].copy_from_slice(&((name.len() * 2) as u32).to_le_bytes());
    for unit in name {
        rename.extend_from_slice(&unit.to_le_bytes());
    }
    write(&mut engine, 0x2000, &rename);
    let identity = info(&mut engine, file, 18, 24);
    assert_eq!(set(&mut engine, file, 3, 0x2000, rename.len() as u32), 1);
    assert_eq!(info(&mut engine, file, 18, 24), identity);
    assert!(
        engine
            .files
            .as_mut()
            .unwrap()
            .vfs_for_test()
            .path_info(&r"C:\renamed.bin".encode_utf16().collect::<Vec<_>>())
            .is_ok()
    );
    assert!(
        engine
            .files
            .as_mut()
            .unwrap()
            .vfs_for_test()
            .path_info(&r"C:\exclusive.bin".encode_utf16().collect::<Vec<_>>())
            .is_err()
    );
}

#[test]
fn captured_disposition_is_reversible_until_marked_description_closes() {
    let mut engine = engine();
    let file = create(&mut engine, r"C:\disposition.bin", 0xc0010000, 0, 0);
    assert_ne!(file, u32::MAX);
    write(&mut engine, 0x2000, &[1]);
    assert_eq!(set(&mut engine, file, 4, 0x2000, 1), 1);
    assert_eq!(info(&mut engine, file, 1, 24)[20], 1);
    write(&mut engine, 0x2000, &[0]);
    assert_eq!(set(&mut engine, file, 4, 0x2000, 1), 1);
    assert_eq!(info(&mut engine, file, 1, 24)[20], 0);
    assert_eq!(
        complete(&mut engine, WindowsApi32::CloseHandle.id(), &[file]),
        1
    );
    let reopened = create(&mut engine, r"C:\disposition.bin", 0xc0010000, 0, 0);
    assert_ne!(reopened, u32::MAX);
    write(&mut engine, 0x2000, &[1]);
    assert_eq!(set(&mut engine, reopened, 4, 0x2000, 1), 1);
    assert_eq!(
        complete(&mut engine, WindowsApi32::CloseHandle.id(), &[reopened]),
        1
    );
    assert!(
        engine
            .files
            .as_mut()
            .unwrap()
            .vfs_for_test()
            .path_info(&r"C:\disposition.bin".encode_utf16().collect::<Vec<_>>())
            .is_err()
    );
}
