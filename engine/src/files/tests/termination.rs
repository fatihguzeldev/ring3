use super::*;

#[test]
fn process_exit_closes_all_description_references_and_preserves_export_data() {
    let mut vfs = vfs();
    let deleted = open(
        &mut vfs,
        "C:\\temporary",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::CreateNew,
            delete_on_close: true,
            ..Default::default()
        },
    );
    vfs.retain_open(deleted.open).unwrap();
    let independent = open(&mut vfs, "C:\\temporary", FileOpenOptions32::default());
    let saved = vfs
        .mount_asset(
            &utf16("C:\\save"),
            AssetRef32 {
                id: 909,
                length: 100,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let ordinary = open(&mut vfs, "C:\\save", FileOpenOptions32::default());
    let attrs = vfs
        .prepare_set_attributes(&utf16("C:\\save"), ATTR_HIDDEN)
        .unwrap();
    vfs.commit_namespace(attrs);
    let FileIo32::NeedBytes(guest) = vfs.prepare_read_at_for(3, saved, 0, 5).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(export) = vfs.read_overlay(saved, vfs.revision(), 0, 5).unwrap() else {
        panic!()
    };
    let prepared = vfs.prepare_process_exit().unwrap();
    vfs.retain_open(ordinary.open).unwrap();
    assert_eq!(prepared.validate(&vfs), Err(FileError32::Stale));
    let prepared = vfs.prepare_process_exit().unwrap();
    vfs.commit_process_exit(prepared);
    for id in [deleted.open, independent.open, ordinary.open] {
        assert_eq!(vfs.open_info(id), Err(FileError32::InvalidHandle));
    }
    assert_eq!(
        vfs.path_info(&utf16("C:\\temporary")),
        Err(FileError32::NotFound)
    );
    assert!(vfs.path_info(&utf16("C:\\save")).is_ok());
    assert_eq!(vfs.supply_bytes(guest, 0, &[1]), Err(FileError32::Stale));
    vfs.supply_bytes(export, 0, &[7; 100]).unwrap();
    let FileIo32::Ready(read) = vfs.read_overlay(saved, vfs.revision(), 0, 5).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), &[7; 5]);
}

#[test]
fn aggregate_process_exit_capacity_and_revision_failures_preserve_all_opens() {
    let mut vfs = Vfs32::new(
        1,
        FileLimits32 {
            nodes: 4,
            ..Default::default()
        },
    )
    .unwrap();
    for index in 0..3 {
        let name = format!("C:\\old{index}");
        let file = open(
            &mut vfs,
            &name,
            FileOpenOptions32 {
                access: FileAccess32::ALL,
                disposition: FileDisposition32::CreateNew,
                delete_on_close: true,
                ..Default::default()
            },
        );
        vfs.close_open(file.open).unwrap();
    }
    let mut retained = Vec::new();
    for name in ["C:\\left", "C:\\right"] {
        retained.push(open(
            &mut vfs,
            name,
            FileOpenOptions32 {
                access: FileAccess32::ALL,
                disposition: FileDisposition32::CreateNew,
                delete_on_close: true,
                ..Default::default()
            },
        ));
    }
    let revision = vfs.revision();
    assert!(matches!(
        vfs.prepare_process_exit(),
        Err(FileError32::Capacity)
    ));
    assert_eq!(vfs.revision(), revision);
    for file in &retained {
        assert!(!vfs.open_info(file.open).unwrap().delete_pending);
    }
    vfs.acknowledge_overlay(vfs.epoch(), revision).unwrap();
    vfs.revision = u64::MAX - 1;
    assert!(matches!(
        vfs.prepare_process_exit(),
        Err(FileError32::Exhausted)
    ));
    for file in &retained {
        assert!(!vfs.open_info(file.open).unwrap().delete_pending);
    }
    vfs.revision = revision;
    let prepared = vfs.prepare_process_exit().unwrap();
    vfs.commit_process_exit(prepared);
    assert_eq!(vfs.revision(), revision + 2);
    for file in retained {
        assert_eq!(vfs.info(file.file), Err(FileError32::InvalidHandle));
    }
}
