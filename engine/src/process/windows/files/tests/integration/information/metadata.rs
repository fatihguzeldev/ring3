use super::*;

#[test]
fn captured_basic_information_changes_all_fields_atomically_and_enforces_granted_handle_rights() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    let mut basic = [0; 40];
    for (index, value) in [111u64, 222, 333, 444].into_iter().enumerate() {
        basic[index * 8..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
    basic[32..36].copy_from_slice(&2u32.to_le_bytes());
    write(&mut engine, 0x2000, &basic);
    assert_eq!(set(&mut engine, file, 0, 0x2000, 40), 1);
    assert_eq!(info(&mut engine, file, 0, 40), basic);
    let before = info(&mut engine, file, 0, 40);
    assert_eq!(set(&mut engine, file, 0, 0x900000, 40), 0);
    assert_eq!(error(&mut engine), 998);
    assert_eq!(info(&mut engine, file, 0, 40), before);
    let read_only = create(&mut engine, r"C:\integration.bin", 0x80000000, 7, 0);
    assert_ne!(read_only, u32::MAX);
    basic[32..36].copy_from_slice(&0x20u32.to_le_bytes());
    write(&mut engine, 0x2000, &basic);
    assert_eq!(set(&mut engine, read_only, 0, 0x2000, 40), 0);
    assert_eq!(error(&mut engine), 5);
    assert_eq!(info(&mut engine, file, 0, 40), before);
}

#[test]
fn captured_end_of_file_preserves_cursor_and_retained_prefix() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    write(&mut engine, 0x2000, b"abcdef");
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::WriteFile.id(),
            &[file, 0x2000, 6, 0x1800, 0]
        ),
        1
    );
    write(&mut engine, 0x2000, &3u64.to_le_bytes());
    assert_eq!(set(&mut engine, file, 6, 0x2000, 8), 1);
    let standard = info(&mut engine, file, 1, 24);
    assert_eq!(u64::from_le_bytes(standard[8..16].try_into().unwrap()), 3);
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::SetFilePointerEx.id(),
            &[file, 0, 0, 0x1800, 1]
        ),
        1
    );
    assert_eq!(
        super::super::super::support::read::<8>(engine.memory().unwrap(), 0x1800),
        6u64.to_le_bytes()
    );
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::SetFilePointerEx.id(),
            &[file, 0, 0, 0, 0]
        ),
        1
    );
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::ReadFile.id(),
            &[file, 0x2100, 6, 0x1800, 0]
        ),
        1
    );
    assert_eq!(
        super::super::super::support::dword(engine.memory().unwrap(), 0x1800),
        3
    );
    assert_eq!(
        super::super::super::support::read::<3>(engine.memory().unwrap(), 0x2100),
        *b"abc"
    );
}

#[test]
fn captured_allocation_reserves_storage_without_moving_eof_or_cursor() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    write(&mut engine, 0x2000, b"abcdef");
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::WriteFile.id(),
            &[file, 0x2000, 6, 0x1800, 0]
        ),
        1
    );
    write(&mut engine, 0x2000, &131072u64.to_le_bytes());
    assert_eq!(set(&mut engine, file, 5, 0x2000, 8), 1);
    let standard = info(&mut engine, file, 1, 24);
    assert_eq!(
        u64::from_le_bytes(standard[..8].try_into().unwrap()),
        131072
    );
    assert_eq!(u64::from_le_bytes(standard[8..16].try_into().unwrap()), 6);
    write(&mut engine, 0x2000, &1u64.to_le_bytes());
    assert_eq!(set(&mut engine, file, 6, 0x2000, 8), 1);
    let standard = info(&mut engine, file, 1, 24);
    assert_eq!(
        u64::from_le_bytes(standard[..8].try_into().unwrap()),
        131072
    );
    assert_eq!(u64::from_le_bytes(standard[8..16].try_into().unwrap()), 1);
    assert_eq!(
        complete(
            &mut engine,
            FileApi32::SetFilePointerEx.id(),
            &[file, 0, 0, 0x1800, 1]
        ),
        1
    );
    assert_eq!(
        super::super::super::support::read::<8>(engine.memory().unwrap(), 0x1800),
        6u64.to_le_bytes()
    );
}

#[test]
fn captured_allocation_is_reported_in_directory_information() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    write(&mut engine, 0x2000, &131072u64.to_le_bytes());
    assert_eq!(set(&mut engine, file, 5, 0x2000, 8), 1);
    write(&mut engine, 0x2000, &1u64.to_le_bytes());
    assert_eq!(set(&mut engine, file, 6, 0x2000, 8), 1);
    super::super::super::support::path(engine.memory.as_mut().unwrap(), 0x1000, r"C:\");
    let directory = complete(
        &mut engine,
        FileApi32::CreateFileW.id(),
        &[0x1000, 0x80000000, 7, 0, 3, 0x02000000, 0],
    );
    assert_ne!(directory, u32::MAX);
    let record = info(&mut engine, directory, 14, 1024);
    assert_eq!(u64::from_le_bytes(record[40..48].try_into().unwrap()), 1);
    assert_eq!(
        u64::from_le_bytes(record[48..56].try_into().unwrap()),
        131072
    );
}
