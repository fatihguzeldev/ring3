use super::*;

#[test]
fn image_section_blocks_every_content_mutation_and_delete_but_preserves_source_rename() {
    let mut vfs = vfs();
    let image = open(
        &mut vfs,
        "C:\\image.dll",
        FileOpenOptions32 {
            access: FileAccess32::READ,
            share: FileShare32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let prepared = vfs
        .prepare_image_opens(&[(image.open, FileSection32::Image)])
        .unwrap();
    vfs.commit_image_opens(prepared);
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_write_at(image.file, 0, b"x"),
        Err(FileError32::SharingViolation)
    ));
    assert!(matches!(
        vfs.prepare_resize(image.file, 0),
        Err(FileError32::SharingViolation)
    ));
    assert!(matches!(
        vfs.prepare_delete(&utf16("C:\\image.dll")),
        Err(FileError32::AccessDenied)
    ));
    assert!(matches!(
        vfs.prepare_open(
            &utf16("C:\\image.dll"),
            FileOpenOptions32 {
                access: FileAccess32::DELETE,
                delete_on_close: true,
                ..Default::default()
            }
        ),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(vfs.revision(), revision);
    let other = open(
        &mut vfs,
        "C:\\other.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(other.open).unwrap();
    assert!(matches!(
        vfs.prepare_move(&utf16("C:\\other.dll"), &utf16("C:\\image.dll"), true),
        Err(FileError32::AccessDenied)
    ));
    let rename = vfs
        .prepare_move(&utf16("C:\\image.dll"), &utf16("C:\\moved.dll"), false)
        .unwrap();
    vfs.commit_namespace(rename);
    assert_eq!(
        vfs.path_info(&utf16("C:\\moved.dll")).unwrap().id,
        image.file
    );
    assert!(vfs.has_file_section(image.file));
    vfs.close_open(image.open).unwrap();
    assert!(!vfs.has_file_section(image.file));
    let deleted = vfs.prepare_delete(&utf16("C:\\moved.dll")).unwrap();
    vfs.commit_namespace(deleted);
}

#[test]
fn image_admission_revision_failure_and_stale_receipt_do_not_adopt_any_open() {
    let mut vfs = vfs();
    let opened = open(
        &mut vfs,
        "C:\\image.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let prepared = vfs
        .prepare_image_opens(&[(opened.open, FileSection32::Image)])
        .unwrap();
    let changed = vfs
        .prepare_set_attributes(&utf16("C:\\image.dll"), ATTR_HIDDEN)
        .unwrap();
    vfs.commit_namespace(changed);
    assert_eq!(prepared.validate(&vfs), Err(FileError32::Stale));
    assert!(!vfs.has_file_section(opened.file));
    let share = vfs.open_options(opened.open).unwrap().share;
    let revision = vfs.revision;
    vfs.revision = u64::MAX;
    assert!(matches!(
        vfs.prepare_image_opens(&[(opened.open, FileSection32::Image)]),
        Err(FileError32::Exhausted)
    ));
    assert!(!vfs.has_file_section(opened.file));
    assert_eq!(vfs.open_options(opened.open).unwrap().share, share);
    vfs.revision = revision;
    assert!(vfs.open_info(opened.open).is_ok());
}

#[test]
fn resource_exclusive_hold_blocks_file_id_writes_and_process_exit_retires_all_sections() {
    let mut vfs = vfs();
    let file = open(
        &mut vfs,
        "C:\\resource.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let prepared = vfs
        .prepare_image_opens(&[(file.open, FileSection32::ExclusiveData)])
        .unwrap();
    vfs.commit_image_opens(prepared);
    assert!(vfs.has_file_section(file.file));
    assert!(matches!(
        vfs.prepare_write_at(file.file, 0, b"x"),
        Err(FileError32::SharingViolation)
    ));
    let exit = vfs.prepare_process_exit().unwrap();
    vfs.commit_process_exit(exit);
    assert_eq!(vfs.open_info(file.open), Err(FileError32::InvalidHandle));
    assert!(matches!(
        vfs.prepare_write_at(file.file, 0, b"x"),
        Ok(FileIo32::Ready(_))
    ));
}

#[test]
fn replace_file_keeps_original_section_alive_but_cannot_discard_a_section_backed_backup() {
    let mut vfs = vfs();
    let original = open(
        &mut vfs,
        "C:\\original.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let held = vfs
        .prepare_image_opens(&[(original.open, FileSection32::Image)])
        .unwrap();
    vfs.commit_image_opens(held);
    let replacement = open(
        &mut vfs,
        "C:\\replacement.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(replacement.open).unwrap();
    let replaced = vfs
        .prepare_replace(
            &utf16("C:\\original.dll"),
            &utf16("C:\\replacement.dll"),
            None,
        )
        .unwrap();
    vfs.commit_namespace(replaced);
    assert_eq!(
        vfs.path_info(&utf16("C:\\original.dll")).unwrap().id,
        replacement.file
    );
    assert_eq!(vfs.open_info(original.open).unwrap().id, original.file);
    assert_eq!(vfs.open_info(original.open).unwrap().links, 0);
    vfs.close_open(original.open).unwrap();

    let replacement2 = open(
        &mut vfs,
        "C:\\replacement2.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(replacement2.open).unwrap();
    let backup = open(
        &mut vfs,
        "C:\\backup.dll",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let held = vfs
        .prepare_image_opens(&[(backup.open, FileSection32::Image)])
        .unwrap();
    vfs.commit_image_opens(held);
    let revision = vfs.revision();
    let paths = ["C:\\original.dll", "C:\\replacement2.dll", "C:\\backup.dll"];
    let identities = paths.map(|path| vfs.path_info(&utf16(path)).unwrap().id);
    let error = vfs
        .prepare_replace(&utf16(paths[0]), &utf16(paths[1]), Some(&utf16(paths[2])))
        .unwrap_err();
    assert_eq!(error.win32(), 1175);
    assert_eq!(vfs.revision(), revision);
    assert_eq!(
        paths.map(|path| vfs.path_info(&utf16(path)).unwrap().id),
        identities
    );
}
