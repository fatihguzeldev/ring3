use super::*;
#[test]
fn duplicate_cursor_and_delete_on_close_follow_open_description() {
    let mut vfs = vfs();
    let first = open(
        &mut vfs,
        "C:\\life",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            delete_on_close: true,
            ..Default::default()
        },
    );
    let second = open(&mut vfs, "C:\\life", FileOpenOptions32::default());
    vfs.retain_open(first.open).unwrap();
    let seek = vfs.prepare_seek(first.open, 9, FileSeek32::Start).unwrap();
    vfs.commit_seek(seek);
    assert_eq!(vfs.position(first.open).unwrap(), 9);
    assert_eq!(vfs.position(second.open).unwrap(), 0);
    vfs.close_open(first.open).unwrap();
    assert!(!vfs.info(first.file).unwrap().delete_pending);
    vfs.close_open(first.open).unwrap();
    assert!(vfs.info(first.file).unwrap().delete_pending);
    assert!(matches!(
        vfs.prepare_open(&utf16("C:\\life"), FileOpenOptions32::default()),
        Err(FileError32::DeletePending)
    ));
    vfs.close_open(second.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\life")),
        Err(FileError32::NotFound)
    );
}
#[test]
fn bidirectional_sharing_and_prepared_failure_are_neutral() {
    let mut vfs = vfs();
    let first = open(
        &mut vfs,
        "C:\\share",
        FileOpenOptions32 {
            access: FileAccess32::WRITE,
            share: FileShare32::READ,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    assert!(matches!(
        vfs.prepare_open(
            &utf16("C:\\share"),
            FileOpenOptions32 {
                share: FileShare32::READ,
                ..Default::default()
            }
        ),
        Err(FileError32::SharingViolation)
    ));
    let metadata = open(
        &mut vfs,
        "C:\\share",
        FileOpenOptions32 {
            access: FileAccess32(0),
            share: FileShare32::ALL,
            ..Default::default()
        },
    );
    assert_eq!(metadata.file, first.file);
    let receipt = vfs.prepare_seek(first.open, 5, FileSeek32::Start).unwrap();
    let change = vfs
        .prepare_set_attributes(&utf16("C:\\share"), ATTR_HIDDEN)
        .unwrap();
    vfs.commit_namespace(change);
    assert_eq!(receipt.validate(&vfs), Err(FileError32::Stale));
    assert_eq!(vfs.position(first.open).unwrap(), 0);
}
#[test]
fn final_close_revision_budget_and_mapping_retention_are_atomic() {
    let mut vfs = vfs();
    let file = open(
        &mut vfs,
        "C:\\held",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            delete_on_close: true,
            ..Default::default()
        },
    );
    vfs.retain_open(file.open).unwrap();
    vfs.close_open(file.open).unwrap();
    assert!(!vfs.info(file.file).unwrap().delete_pending);
    let saved = vfs.revision;
    vfs.revision = u64::MAX - 1;
    assert_eq!(
        vfs.check_close_open_after_mutations(file.open, 1),
        Err(FileError32::Exhausted)
    );
    assert!(!vfs.info(file.file).unwrap().delete_pending);
    vfs.revision = u64::MAX;
    assert_eq!(vfs.close_open(file.open), Err(FileError32::Exhausted));
    assert!(vfs.open_info(file.open).is_ok());
    vfs.revision = saved;
    vfs.close_open(file.open).unwrap();
    assert_eq!(vfs.info(file.file), Err(FileError32::InvalidHandle));
    let next = open(
        &mut vfs,
        "C:\\next",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    assert!(next.file.0 > file.file.0);
    assert_eq!(vfs.info(file.file), Err(FileError32::InvalidHandle));
}
#[test]
fn readonly_directory_metadata_does_not_deny_delete_authority() {
    let mut vfs = vfs();
    vfs.mount_directory(&utf16("C:\\folder"), ATTR_READONLY, FileTimes32::default())
        .unwrap();
    let directory = open(
        &mut vfs,
        "C:\\folder",
        FileOpenOptions32 {
            access: FileAccess32::DELETE,
            directory: true,
            ..Default::default()
        },
    );
    let delete = vfs.prepare_remove_directory(&utf16("C:\\folder")).unwrap();
    vfs.commit_namespace(delete);
    assert!(vfs.open_info(directory.open).unwrap().delete_pending);
    vfs.close_open(directory.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\folder")),
        Err(FileError32::NotFound)
    );
}
#[test]
fn readonly_metadata_preserves_existing_granted_write_handles() {
    let mut vfs = vfs();
    let file = open(
        &mut vfs,
        "C:\\readonly",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            attributes: ATTR_READONLY,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(file.open, b"created").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    assert!(matches!(
        vfs.prepare_open(
            &utf16("C:\\readonly"),
            FileOpenOptions32 {
                access: FileAccess32::WRITE,
                ..Default::default()
            }
        ),
        Err(FileError32::AccessDenied)
    ));
    assert!(matches!(
        vfs.prepare_delete(&utf16("C:\\readonly")),
        Err(FileError32::AccessDenied)
    ));
    let resize = vfs.prepare_resize(file.file, 2).unwrap();
    vfs.commit_resize(resize);
    assert_eq!(vfs.info(file.file).unwrap().size, 2);
}
#[test]
fn directory_delete_on_close_never_orphans_children_or_rejects_valid_close() {
    let mut vfs = vfs();
    vfs.mount_asset(
        &utf16("C:\\nonempty\\child"),
        AssetRef32 { id: 90, length: 0 },
        ATTR_NORMAL,
        FileTimes32::default(),
    )
    .unwrap();
    let options = FileOpenOptions32 {
        access: FileAccess32::DELETE,
        directory: true,
        delete_on_close: true,
        ..Default::default()
    };
    let directory = open(&mut vfs, "C:\\nonempty", options);
    vfs.close_open(directory.open).unwrap();
    assert!(vfs.path_info(&utf16("C:\\nonempty\\child")).is_ok());
    assert!(vfs.path_info(&utf16("C:\\nonempty")).is_ok());
    vfs.mount_directory(
        &utf16("C:\\late-child"),
        ATTR_DIRECTORY,
        FileTimes32::default(),
    )
    .unwrap();
    let directory = open(&mut vfs, "C:\\late-child", options);
    let child = open(
        &mut vfs,
        "C:\\late-child\\child",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(directory.open).unwrap();
    assert!(vfs.open_info(child.open).is_ok());
    assert!(
        !vfs.path_info(&utf16("C:\\late-child"))
            .unwrap()
            .delete_pending
    );
    vfs.mount_directory(&utf16("C:\\empty"), ATTR_DIRECTORY, FileTimes32::default())
        .unwrap();
    let directory = open(&mut vfs, "C:\\empty", options);
    let held = open(
        &mut vfs,
        "C:\\empty",
        FileOpenOptions32 {
            directory: true,
            ..Default::default()
        },
    );
    vfs.close_open(directory.open).unwrap();
    assert!(vfs.open_info(held.open).unwrap().delete_pending);
    assert!(matches!(
        vfs.prepare_open(
            &utf16("C:\\empty\\child"),
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateNew,
                ..Default::default()
            }
        ),
        Err(FileError32::PathNotFound)
    ));
    vfs.close_open(held.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\empty")),
        Err(FileError32::NotFound)
    );
}
#[test]
fn grouped_close_preflight_counts_combined_deletions_and_final_references() {
    let mut vfs = Vfs32::new(
        1,
        FileLimits32 {
            nodes: 4,
            ..Default::default()
        },
    )
    .unwrap();
    for index in 0..3 {
        let file = open(
            &mut vfs,
            &format!("C:\\history{index}"),
            FileOpenOptions32 {
                disposition: FileDisposition32::CreateNew,
                ..Default::default()
            },
        );
        vfs.close_open(file.open).unwrap();
        let delete = vfs
            .prepare_delete(&utf16(&format!("C:\\history{index}")))
            .unwrap();
        vfs.commit_namespace(delete);
    }
    let first = open(
        &mut vfs,
        "C:\\first",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            delete_on_close: true,
            ..Default::default()
        },
    );
    let second = open(
        &mut vfs,
        "C:\\second",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.retain_open(first.open).unwrap();
    vfs.retain_open(second.open).unwrap();
    assert_eq!(
        vfs.check_close_open_group(
            &[(first.open, 1), (first.open, 1), (second.open, 2)],
            1,
            &[second.file]
        ),
        Err(FileError32::Capacity)
    );
    assert_eq!(vfs.open(first.open).unwrap().references, 2);
    assert!(!vfs.info(second.file).unwrap().delete_pending);
    vfs.acknowledge_overlay(1, vfs.revision()).unwrap();
    vfs.check_close_open_group(&[(first.open, 2), (second.open, 2)], 1, &[second.file])
        .unwrap();
    let cancel = vfs.prepare_cancel_copy_destination(second.file).unwrap();
    vfs.commit_namespace(cancel);
    for open in [first.open, second.open] {
        vfs.close_open(open).unwrap();
        vfs.close_open(open).unwrap();
    }
    assert_eq!(vfs.info(first.file), Err(FileError32::InvalidHandle));
    assert_eq!(vfs.info(second.file), Err(FileError32::InvalidHandle));
}
#[test]
fn current_directory_keeps_implicit_delete_sharing_authority() {
    let mut vfs = vfs();
    vfs.mount_directory(
        &utf16("C:\\working"),
        ATTR_DIRECTORY,
        FileTimes32::default(),
    )
    .unwrap();
    vfs.mount_directory(&utf16("C:\\other"), ATTR_DIRECTORY, FileTimes32::default())
        .unwrap();
    let current = vfs
        .prepare_current_directory(&utf16("C:\\working"))
        .unwrap();
    vfs.commit_namespace(current);
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_move(&utf16("C:\\working"), &utf16("C:\\renamed"), false),
        Err(FileError32::SharingViolation)
    ));
    assert!(matches!(
        vfs.prepare_open(
            &utf16("C:\\working"),
            FileOpenOptions32 {
                access: FileAccess32::DELETE,
                directory: true,
                delete_on_close: true,
                ..Default::default()
            }
        ),
        Err(FileError32::SharingViolation)
    ));
    assert_eq!(vfs.revision(), revision);
    assert_eq!(vfs.current_directory(), utf16("C:\\working"));
    let deleting = open(
        &mut vfs,
        "C:\\other",
        FileOpenOptions32 {
            access: FileAccess32::DELETE,
            directory: true,
            ..Default::default()
        },
    );
    assert!(matches!(
        vfs.prepare_current_directory(&utf16("C:\\other")),
        Err(FileError32::SharingViolation)
    ));
    assert_eq!(vfs.current_directory(), utf16("C:\\working"));
    vfs.close_open(deleting.open).unwrap();
    let change = vfs.prepare_current_directory(&utf16("C:\\other")).unwrap();
    vfs.commit_namespace(change);
    let renamed = vfs
        .prepare_move(&utf16("C:\\working"), &utf16("C:\\renamed"), false)
        .unwrap();
    vfs.commit_namespace(renamed);
    assert!(vfs.path_info(&utf16("C:\\renamed")).is_ok());
}
