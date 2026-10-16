use super::*;

#[test]
fn basic_time_suppression_is_per_description_and_can_be_restored() {
    let mut vfs = vfs();
    let file = open(
        &mut vfs,
        "C:\\times",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let basic = vfs
        .prepare_basic_information(file.open, [11, 22, 33, 44], ATTR_HIDDEN)
        .unwrap();
    vfs.commit_namespace(basic);
    let suppress = vfs
        .prepare_basic_information(file.open, [0, u64::MAX, u64::MAX, u64::MAX], 0)
        .unwrap();
    vfs.commit_namespace(suppress);
    vfs.set_time(100).unwrap();
    let FileIo32::Ready(write) = vfs.prepare_write(file.open, b"x").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    assert_eq!(
        vfs.info(file.file).unwrap().times,
        FileTimes32 {
            created: 11,
            accessed: 22,
            written: 33
        }
    );
    assert_eq!(vfs.change_time(file.file), Ok(44));
    let restore = vfs
        .prepare_basic_information(file.open, [0, u64::MAX - 1, u64::MAX - 1, u64::MAX - 1], 0)
        .unwrap();
    vfs.commit_namespace(restore);
    vfs.set_time(200).unwrap();
    let FileIo32::Ready(write) = vfs.prepare_write(file.open, b"y").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    assert_eq!(vfs.info(file.file).unwrap().times.written, 200);
    assert_eq!(vfs.change_time(file.file), Ok(200));
}

#[test]
fn explicit_allocation_owns_bounded_zero_storage_and_cannot_revive_truncated_data() {
    let mut vfs = Vfs32::new(
        1,
        FileLimits32 {
            overlay_pages: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let file = open(
        &mut vfs,
        "C:\\allocation",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(file.open, b"abc").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    let allocation = vfs.prepare_allocation(file.open, 131072).unwrap();
    vfs.commit_allocation(allocation);
    assert_eq!(vfs.overlay_pages, 2);
    assert_eq!(vfs.allocation_size(file.file), Ok(131072));
    let before = (
        vfs.revision(),
        vfs.info(file.file).unwrap(),
        vfs.content_version(file.file).unwrap(),
    );
    assert!(matches!(
        vfs.prepare_allocation(file.open, 196608),
        Err(FileError32::DiskFull)
    ));
    assert_eq!(
        (
            vfs.revision(),
            vfs.info(file.file).unwrap(),
            vfs.content_version(file.file).unwrap()
        ),
        before
    );
    let eof = vfs.prepare_resize_open(file.open, 1).unwrap();
    vfs.commit_resize(eof);
    assert_eq!(vfs.allocation_size(file.file), Ok(131072));
    assert_eq!(vfs.overlay_pages, 2);
    let grow = vfs.prepare_resize_open(file.open, 65537).unwrap();
    vfs.commit_resize(grow);
    let FileIo32::Ready(read) = vfs.prepare_read_at(file.file, 0, 65537).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes()[0], b'a');
    assert!(read.bytes()[1..].iter().all(|&byte| byte == 0));
    let shrink = vfs.prepare_allocation(file.open, 1).unwrap();
    vfs.commit_allocation(shrink);
    assert_eq!(vfs.allocation_size(file.file), Ok(65536));
    assert_eq!(vfs.overlay_pages, 1);
    assert_eq!(vfs.info(file.file).unwrap().size, 1);
}

#[test]
fn information_receipts_reject_stale_or_exhausted_publication_without_changes() {
    let mut vfs = vfs();
    let file = open(
        &mut vfs,
        "C:\\current",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let basic = vfs
        .prepare_basic_information(file.open, [1, 2, 3, 4], ATTR_HIDDEN)
        .unwrap();
    let allocation = vfs.prepare_allocation(file.open, 65536).unwrap();
    let disposition = vfs.prepare_disposition(file.open, true).unwrap();
    let changed = vfs
        .prepare_set_attributes(&utf16("C:\\current"), ATTR_SYSTEM)
        .unwrap();
    vfs.commit_namespace(changed);
    assert_eq!(basic.validate(&vfs), Err(FileError32::Stale));
    assert_eq!(allocation.validate(&vfs), Err(FileError32::Stale));
    assert_eq!(disposition.validate(&vfs), Err(FileError32::Stale));
    let before = vfs.info(file.file).unwrap();
    vfs.revision = u64::MAX;
    assert!(matches!(
        vfs.prepare_basic_information(file.open, [1, 2, 3, 4], ATTR_HIDDEN),
        Err(FileError32::Exhausted)
    ));
    assert!(matches!(
        vfs.prepare_allocation(file.open, 65536),
        Err(FileError32::Exhausted)
    ));
    assert!(matches!(
        vfs.prepare_disposition(file.open, true),
        Err(FileError32::Exhausted)
    ));
    assert_eq!(vfs.info(file.file).unwrap(), before);
    assert_eq!(vfs.overlay_pages, 0);
}

#[test]
fn disposition_description_close_preserves_other_descriptions_and_irreversible_deletion() {
    let mut vfs = vfs();
    let marked = open(
        &mut vfs,
        "C:\\delete",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let other = open(
        &mut vfs,
        "C:\\delete",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            ..Default::default()
        },
    );
    let delete = vfs.prepare_disposition(marked.open, true).unwrap();
    vfs.commit_namespace(delete);
    vfs.retain_open(marked.open).unwrap();
    vfs.close_open(marked.open).unwrap();
    assert!(vfs.open_info(other.open).unwrap().delete_pending);
    let undo = vfs.prepare_disposition(marked.open, false).unwrap();
    vfs.commit_namespace(undo);
    assert!(!vfs.open_info(other.open).unwrap().delete_pending);
    let delete = vfs.prepare_disposition(marked.open, true).unwrap();
    vfs.commit_namespace(delete);
    vfs.close_open(marked.open).unwrap();
    let undo = vfs.prepare_disposition(other.open, false).unwrap();
    vfs.commit_namespace(undo);
    assert!(vfs.open_info(other.open).unwrap().delete_pending);
    vfs.close_open(other.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\delete")),
        Err(FileError32::NotFound)
    );
}
