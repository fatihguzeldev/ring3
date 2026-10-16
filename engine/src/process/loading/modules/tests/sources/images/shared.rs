use super::*;
use crate::loader::ModuleError32;

#[test]
fn shared_datafile_accepts_an_existing_writer_while_image_profiles_refuse_it() {
    for flags in [2, 0x20, 0x40] {
        let bytes = image(true, 0x600000, None, None);
        let path = r"C:\app\shared.dll".encode_utf16().collect::<Vec<_>>();
        let assets = [(r"C:\app\shared.dll", bytes.as_slice())];
        let mut engine = filesystem(&assets);
        engine
            .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
            .unwrap();
        engine
            .start_module_process(r"C:\app\main.exe", None, Default::default())
            .unwrap();
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
        let version = engine.provider_path_version(&path).unwrap();
        let before = engine.memory().unwrap().transaction_version();
        let result = engine.load_resource_only_module(r"C:\app\shared.dll", flags);
        if flags == 2 {
            let (handle, waits) = finish(&mut engine, result.unwrap(), &assets);
            assert_eq!(waits, 1);
            engine.free_resource_only_module(handle).unwrap();
        } else {
            assert_eq!(result, Err(HostError::File(FileError32::SharingViolation)));
            assert_eq!(engine.memory().unwrap().transaction_version(), before);
            assert!(engine.pending_module_source().is_none());
        }
        assert_eq!(engine.provider_path_version(&path).unwrap(), version);
        let files = engine.files.as_mut().unwrap().vfs_for_test();
        assert_eq!(files.open_info(writer.open).unwrap().id, version.file);
        files.close_open(writer.open).unwrap();
    }
}

#[test]
fn a_changed_shared_source_becomes_ready_for_exact_failure_cleanup_instead_of_adopting_bytes() {
    let mut bytes = image(true, 0x600000, None, None);
    bytes.resize(0x11000, 0);
    let path = r"C:\app\shared.dll".encode_utf16().collect::<Vec<_>>();
    let mut engine = filesystem(&[(r"C:\app\shared.dll", bytes.as_slice())]);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
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
    let before = engine.memory().unwrap().transaction_version();
    let ModuleOperation32::NeedBytes { request } = engine
        .load_resource_only_module(r"C:\app\shared.dll", 2)
        .unwrap()
    else {
        panic!("lazy source")
    };
    let wait = engine.pending_module_source().unwrap();
    let assets = [(r"C:\app\shared.dll", bytes.as_slice())];
    supply(&mut engine, request, &assets);
    let ModuleOperation32::NeedBytes { request } =
        engine.resume_module_source(wait.epoch, wait.token).unwrap()
    else {
        panic!("the first original chunk is retained before waiting for the second")
    };
    assert_eq!(request.offset, 65536);
    supply(&mut engine, request, &assets);
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let crate::files::FileIo32::Ready(write) =
        files.prepare_write_at(writer.file, 0x201, &[0x5a]).unwrap()
    else {
        panic!("the supplied page is available to the writer")
    };
    files.commit_write(write);
    assert!(engine.pending_module_source().unwrap().ready);
    assert_eq!(
        engine.resume_module_source(wait.epoch, wait.token),
        Err(HostError::Module(ModuleError32::StalePlan))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.pending_module_source().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    assert!(!engine.modules.catalog.contains(r"C:\app\shared.dll"));
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    assert_eq!(files.open_info(writer.open).unwrap().id, writer.file);
    files.close_open(writer.open).unwrap();
    let deleted = files.prepare_delete(&path).unwrap();
    files.commit_namespace(deleted);
}
