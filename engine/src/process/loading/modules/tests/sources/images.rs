use super::*;
use crate::files::{FileAccess32, FileDisposition32, FileError32, FileOpenOptions32, FileShare32};

#[test]
fn loaded_vfs_image_retains_file_identity_until_its_last_module_reference_is_released() {
    let bytes = image(true, 0x600000, None, None);
    let path = r"C:\app\plugin.dll".encode_utf16().collect::<Vec<_>>();
    let renamed = r"C:\app\renamed.dll".encode_utf16().collect::<Vec<_>>();
    let assets = [(r"C:\app\plugin.dll", bytes.as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let first = engine
        .load_library_module(r"C:\app\plugin.dll", None)
        .unwrap();
    assert_eq!(finish(&mut engine, first, &assets), (0x600000, 1));
    assert_eq!(
        engine
            .load_library_module(r"C:\app\plugin.dll", None)
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    let retired = engine
        .files
        .as_ref()
        .unwrap()
        .prepare_thread_exit(engine.memory().unwrap(), 1, &[], &[], false)
        .unwrap();
    engine
        .files
        .as_mut()
        .unwrap()
        .commit_thread_exit(1, retired);
    // a module owns its source after loading; retiring the loading thread's files cannot release it.
    let version = engine.provider_path_version(&path).unwrap();
    assert_eq!(
        engine.replace_provider_file(&path, b"replacement"),
        Err(HostError::File(FileError32::SharingViolation))
    );
    assert_eq!(engine.provider_path_version(&path).unwrap(), version);
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    assert!(matches!(
        files.prepare_open(
            &path,
            FileOpenOptions32 {
                access: FileAccess32::WRITE,
                share: FileShare32::ALL,
                disposition: FileDisposition32::OpenExisting,
                ..Default::default()
            }
        ),
        Err(FileError32::SharingViolation)
    ));
    assert!(matches!(
        files.prepare_delete(&path),
        Err(FileError32::AccessDenied)
    ));
    let moved = files.prepare_move(&path, &renamed, false).unwrap();
    files.commit_namespace(moved);
    assert_eq!(files.path_info(&renamed).unwrap().id, version.file);
    assert!(matches!(
        files.prepare_delete(&renamed),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    assert!(matches!(
        engine
            .files
            .as_mut()
            .unwrap()
            .vfs_for_test()
            .prepare_delete(&renamed),
        Err(FileError32::AccessDenied)
    ));
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let deleted = files.prepare_delete(&renamed).unwrap();
    files.commit_namespace(deleted);
    assert!(matches!(
        files.path_info(&renamed),
        Err(FileError32::NotFound)
    ));
}

#[test]
fn image_resource_and_exclusive_datafile_keep_write_exclusion_until_free_library() {
    for flags in [0x20, 0x40] {
        let bytes = image(true, 0x600000, None, None);
        let path = r"C:\app\resources.dll".encode_utf16().collect::<Vec<_>>();
        let assets = [(r"C:\app\resources.dll", bytes.as_slice())];
        let mut engine = filesystem(&assets);
        engine
            .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
            .unwrap();
        engine
            .start_module_process(r"C:\app\main.exe", None, Default::default())
            .unwrap();
        let first = engine
            .load_resource_only_module(r"C:\app\resources.dll", flags)
            .unwrap();
        let (handle, waits) = finish(&mut engine, first, &assets);
        assert_eq!(waits, 1);
        let version = engine.provider_path_version(&path).unwrap();
        assert_eq!(
            engine.replace_provider_file(&path, b"replacement"),
            Err(HostError::File(FileError32::SharingViolation)),
            "flags {flags:#x}"
        );
        assert_eq!(engine.provider_path_version(&path).unwrap(), version);
        engine.free_resource_only_module(handle).unwrap();
        assert_eq!(
            engine.replace_provider_file(&path, b"replacement").unwrap(),
            11
        );
    }
}

#[test]
fn ordinary_datafile_allows_writes_but_keeps_delete_exclusion_until_free_library() {
    let bytes = image(true, 0x600000, None, None);
    let path = r"C:\app\resources.dll".encode_utf16().collect::<Vec<_>>();
    let assets = [(r"C:\app\resources.dll", bytes.as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let operation = engine
        .load_resource_only_module(r"C:\app\resources.dll", 2)
        .unwrap();
    let (handle, waits) = finish(&mut engine, operation, &assets);
    assert_eq!(waits, 1);
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let writer = files
        .prepare_open(
            &path,
            FileOpenOptions32 {
                access: FileAccess32::WRITE,
                share: FileShare32::ALL,
                ..Default::default()
            },
        )
        .unwrap();
    let writer = files.commit_open(writer);
    files.close_open(writer.open).unwrap();
    assert!(matches!(
        files.prepare_delete(&path),
        Err(FileError32::AccessDenied)
    ));
    engine.free_resource_only_module(handle).unwrap();
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let deleted = files.prepare_delete(&path).unwrap();
    files.commit_namespace(deleted);
    assert_eq!(files.path_info(&path), Err(FileError32::NotFound));
}
