use super::*;
#[test]
fn mounted_parents_case_and_utf16_identity_are_preserved() {
    let mut vfs = vfs();
    let file = vfs
        .mount_asset(
            &utf16("C:\\one\\two\\three\\game.dat"),
            AssetRef32 { id: 1, length: 4 },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    assert_eq!(
        vfs.path_info(&utf16("c:/ONE/two/three/GAME.dat"))
            .unwrap()
            .id,
        file
    );
    for path in ["C:\\one", "C:\\one\\two", "C:\\one\\two\\three"] {
        assert_ne!(
            vfs.path_info(&utf16(path)).unwrap().attributes & ATTR_DIRECTORY,
            0
        );
    }
    let name = [67, 58, 92, 0xd800, 46, 98];
    vfs.mount_asset(
        &name,
        AssetRef32 { id: 2, length: 0 },
        ATTR_NORMAL,
        FileTimes32::default(),
    )
    .unwrap();
    assert!(vfs.path_info(&name).is_ok());
    assert_eq!(
        vfs.full_path(&utf16("C:\\..\\one\\.\\two\\..\\x. "))
            .unwrap(),
        utf16("C:\\one\\x")
    );
    assert_eq!(
        vfs.full_path(&utf16("C:\\NUL.txt")),
        Err(FileError32::Unsupported)
    );
}
#[test]
fn copy_snapshot_move_identity_and_overlay_tombstones() {
    let mut vfs = vfs();
    let original = open(
        &mut vfs,
        "C:\\source",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(original.open, b"before").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    let FileIo32::Ready(copy) = vfs
        .prepare_copy(&utf16("C:\\source"), &utf16("C:\\copy"), true)
        .unwrap()
    else {
        panic!()
    };
    vfs.commit_namespace(copy);
    let FileIo32::Ready(write) = vfs.prepare_write_at(original.file, 0, b"after!").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    let copied = vfs.path_info(&utf16("C:\\copy")).unwrap().id;
    let FileIo32::Ready(read) = vfs.prepare_read_at(copied, 0, 6).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), b"before");
    let rename = vfs
        .prepare_move(&utf16("C:\\source"), &utf16("C:\\moved"), false)
        .unwrap();
    vfs.commit_namespace(rename);
    assert_eq!(
        vfs.path_info(&utf16("C:\\moved")).unwrap().id,
        original.file
    );
    assert_eq!(vfs.file_path(original.file).unwrap(), utf16("C:\\moved"));
    assert!(
        (0..8)
            .filter_map(|i| vfs.overlay_entry(i).unwrap())
            .any(|e| e.kind == OverlayKind32::Deleted && e.path == utf16("C:\\source"))
    );
}
#[test]
fn rename_recreation_and_case_only_moves_do_not_export_live_path_deletion() {
    let mut vfs = vfs();
    let first = open(
        &mut vfs,
        "C:\\first",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(first.open).unwrap();
    let second = open(
        &mut vfs,
        "C:\\second",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.close_open(second.open).unwrap();
    let delete = vfs.prepare_delete(&utf16("C:\\second")).unwrap();
    vfs.commit_namespace(delete);
    let rename = vfs
        .prepare_move(&utf16("C:\\first"), &utf16("C:\\second"), false)
        .unwrap();
    vfs.commit_namespace(rename);
    assert!(!(0..8).filter_map(|i|vfs.overlay_entry(i).unwrap()).any(|e|e.kind==OverlayKind32::Deleted&&path::fold(&e.path)==utf16("C:\\SECOND")));
    let rename = vfs
        .prepare_move(&utf16("C:\\second"), &utf16("C:\\SECOND"), false)
        .unwrap();
    vfs.commit_namespace(rename);
    assert_eq!(vfs.path_info(&utf16("C:\\second")).unwrap().id, first.file);
    assert!(!(0..8).filter_map(|i|vfs.overlay_entry(i).unwrap()).any(|e|e.kind==OverlayKind32::Deleted&&path::fold(&e.path)==utf16("C:\\SECOND")));
}
#[test]
fn enumeration_applies_windows_dos_wildcards_without_crossing_directories() {
    let mut vfs = vfs();
    for (index, name) in ["a.txt", "ab.txt", "abc.txt", "plain", "nested\\hidden.txt"]
        .iter()
        .enumerate()
    {
        vfs.mount_asset(
            &utf16(&format!("C:\\data\\{name}")),
            AssetRef32 {
                id: index as u64 + 40,
                length: 0,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    }
    let names = |vfs: &Vfs32, pattern: &str| {
        vfs.find(&utf16(pattern))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&vfs, "C:\\data\\a??.txt").len(), 3);
    assert!(names(&vfs, "C:\\data\\*.*").contains(&utf16("plain")));
    assert_eq!(
        names(&vfs, "C:\\data\\*."),
        vec![utf16("nested"), utf16("plain")]
    );
    assert_eq!(names(&vfs, "C:\\data\\*.txt").len(), 3);
}
#[test]
fn copy_continuation_targets_held_identities_after_name_replacement() {
    let mut vfs = vfs();
    let source = open(
        &mut vfs,
        "C:\\source",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let target = open(
        &mut vfs,
        "C:\\target",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(source.open, b"original").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    let attrs = vfs
        .prepare_set_attributes(&utf16("C:\\source"), ATTR_READONLY)
        .unwrap();
    vfs.commit_namespace(attrs);
    for (old, new) in [
        ("C:\\source", "C:\\moved-source"),
        ("C:\\target", "C:\\moved-target"),
    ] {
        let rename = vfs.prepare_move(&utf16(old), &utf16(new), false).unwrap();
        vfs.commit_namespace(rename);
    }
    let replacement_source = open(
        &mut vfs,
        "C:\\source",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let replacement_target = open(
        &mut vfs,
        "C:\\target",
        FileOpenOptions32 {
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let copy = vfs.prepare_copy_file(source.file, target.file).unwrap();
    vfs.commit_namespace(copy);
    let FileIo32::Ready(read) = vfs.prepare_read_at(target.file, 0, 8).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), b"original");
    assert_eq!(vfs.info(replacement_source.file).unwrap().size, 0);
    assert_eq!(vfs.info(replacement_target.file).unwrap().size, 0);
    let cancel = vfs.prepare_cancel_copy_destination(target.file).unwrap();
    vfs.commit_namespace(cancel);
    vfs.close_open(target.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\moved-target")),
        Err(FileError32::NotFound)
    );
    assert_eq!(
        vfs.path_info(&utf16("C:\\target")).unwrap().id,
        replacement_target.file
    );
}
#[test]
fn replace_atomically_preserves_replacement_id_original_creation_and_backup_identity() {
    let mut vfs = vfs();
    vfs.set_time(100).unwrap();
    let original = open(
        &mut vfs,
        "C:\\original",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(original.open, b"old").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    vfs.set_time(200).unwrap();
    let replacement = open(
        &mut vfs,
        "C:\\replacement",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    let FileIo32::Ready(write) = vfs.prepare_write(replacement.open, b"new").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    vfs.close_open(replacement.open).unwrap();
    let replacement_metadata = open(
        &mut vfs,
        "C:\\replacement",
        FileOpenOptions32 {
            access: FileAccess32(0),
            ..Default::default()
        },
    );
    vfs.set_time(300).unwrap();
    let receipt = vfs
        .prepare_replace(
            &utf16("C:\\original"),
            &utf16("C:\\replacement"),
            Some(&utf16("C:\\backup")),
        )
        .unwrap();
    vfs.commit_namespace(receipt);
    assert_eq!(
        vfs.path_info(&utf16("C:\\original")).unwrap().id,
        replacement.file
    );
    assert_eq!(
        vfs.path_info(&utf16("C:\\backup")).unwrap().id,
        original.file
    );
    assert_eq!(vfs.info(replacement.file).unwrap().times.created, 100);
    assert_eq!(vfs.change_time(replacement.file).unwrap(), 300);
    assert_eq!(
        vfs.opened_path(original.open).unwrap(),
        utf16("C:\\original")
    );
    assert_eq!(vfs.file_path(original.file).unwrap(), utf16("C:\\backup"));
    assert_eq!(
        vfs.opened_path(replacement_metadata.open).unwrap(),
        utf16("C:\\replacement")
    );
    assert_eq!(
        vfs.path_info(&utf16("C:\\replacement")),
        Err(FileError32::NotFound)
    );
    let FileIo32::Ready(old) = vfs.prepare_read_at(original.file, 0, 3).unwrap() else {
        panic!()
    };
    assert_eq!(old.bytes(), b"old");
    let FileIo32::Ready(new) = vfs.prepare_read_at(replacement.file, 0, 3).unwrap() else {
        panic!()
    };
    assert_eq!(new.bytes(), b"new");
}
#[test]
fn metadata_change_time_and_last_write_time_are_distinct() {
    let mut vfs = vfs();
    vfs.set_time(11).unwrap();
    let file = open(
        &mut vfs,
        "C:\\times",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            ..Default::default()
        },
    );
    vfs.set_time(22).unwrap();
    let attrs = vfs
        .prepare_set_attributes(&utf16("C:\\times"), ATTR_HIDDEN)
        .unwrap();
    vfs.commit_namespace(attrs);
    assert_eq!(vfs.change_time(file.file).unwrap(), 22);
    assert_eq!(vfs.info(file.file).unwrap().times.written, 11);
    vfs.set_time(33).unwrap();
    let suppression = vfs
        .prepare_file_times(file.open, None, None, Some(u64::MAX))
        .unwrap();
    vfs.commit_namespace(suppression);
    let FileIo32::Ready(write) = vfs.prepare_write(file.open, b"x").unwrap() else {
        panic!()
    };
    vfs.commit_write(write);
    assert_eq!(vfs.info(file.file).unwrap().times.written, 11);
    assert_eq!(vfs.change_time(file.file).unwrap(), 33);
}
#[test]
fn host_mount_rejects_pending_parent_and_conflicting_live_asset_identity() {
    let mut vfs = vfs();
    vfs.mount_directory(&utf16("C:\\parent"), ATTR_DIRECTORY, FileTimes32::default())
        .unwrap();
    let handle = open(
        &mut vfs,
        "C:\\parent",
        FileOpenOptions32 {
            directory: true,
            ..Default::default()
        },
    );
    let delete = vfs.prepare_remove_directory(&utf16("C:\\parent")).unwrap();
    vfs.commit_namespace(delete);
    let revision = vfs.revision();
    assert_eq!(
        vfs.mount_asset(
            &utf16("C:\\parent\\child"),
            AssetRef32 { id: 100, length: 4 },
            ATTR_NORMAL,
            FileTimes32::default()
        ),
        Err(FileError32::DeletePending)
    );
    assert_eq!(
        vfs.mount_directory(&utf16("C:\\parent"), ATTR_DIRECTORY, FileTimes32::default()),
        Err(FileError32::DeletePending)
    );
    assert_eq!(vfs.revision(), revision);
    vfs.close_open(handle.open).unwrap();
    assert_eq!(
        vfs.path_info(&utf16("C:\\parent\\child")),
        Err(FileError32::NotFound)
    );
    vfs.mount_asset(
        &utf16("C:\\first"),
        AssetRef32 { id: 101, length: 4 },
        ATTR_NORMAL,
        FileTimes32::default(),
    )
    .unwrap();
    let revision = vfs.revision();
    assert_eq!(
        vfs.mount_asset(
            &utf16("C:\\second"),
            AssetRef32 { id: 101, length: 8 },
            ATTR_NORMAL,
            FileTimes32::default()
        ),
        Err(FileError32::InvalidParameter)
    );
    assert_eq!(vfs.revision(), revision);
    assert_eq!(
        vfs.path_info(&utf16("C:\\second")),
        Err(FileError32::NotFound)
    );
}
