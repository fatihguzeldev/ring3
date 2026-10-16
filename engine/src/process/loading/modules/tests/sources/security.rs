use super::*;
use crate::{
    files::{FileAccess32, FileDisposition32, FileError32, FileOpenOptions32, FileShare32},
    windows::security::{Acl32, Dacl32},
};

fn cached_library() -> EngineInstance {
    let bytes = image(true, 0x600000, None, None);
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
    engine.free_library_module(0x600000).unwrap();
    engine
}

#[test]
fn cached_unloaded_vfs_module_rechecks_current_acl_before_installation() {
    let mut engine = cached_library();
    let path = r"C:\app\plugin.dll".encode_utf16().collect::<Vec<_>>();
    let token = engine.windows_security.token().clone();
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let file = files.path_info(&path).unwrap().id;
    let mut descriptor = files.file_security(file, 4, &token, None).unwrap();
    descriptor.dacl = Dacl32::Acl(Acl32 {
        revision: 2,
        aces: Vec::new(),
    });
    let security = files
        .prepare_file_security(file, 4, descriptor, &token, None)
        .unwrap();
    files.commit_file_security(security);
    assert_eq!(
        engine.open_provider_file(&path, false),
        Err(HostError::File(FileError32::AccessDenied))
    );
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.load_library_module(r"C:\app\plugin.dll", None),
        Err(HostError::File(FileError32::AccessDenied))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine.module_handle(Some(r"C:\app\plugin.dll")).unwrap(),
        None
    );
    assert!(engine.pending_module_source().is_none());
    assert!(engine.loader_initialization_owner().is_none());
}

#[test]
fn cached_unloaded_vfs_module_rechecks_sharing_and_keeps_conflicting_open_alive() {
    let mut engine = cached_library();
    let path = r"C:\app\plugin.dll".encode_utf16().collect::<Vec<_>>();
    let files = engine.files.as_mut().unwrap().vfs_for_test();
    let open = files
        .prepare_open(
            &path,
            FileOpenOptions32 {
                access: FileAccess32::WRITE,
                share: FileShare32(0),
                disposition: FileDisposition32::OpenExisting,
                ..Default::default()
            },
        )
        .unwrap();
    let open = files.commit_open(open).open;
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.load_library_module(r"C:\app\plugin.dll", None),
        Err(HostError::File(FileError32::SharingViolation))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert!(
        engine
            .files
            .as_mut()
            .unwrap()
            .vfs_for_test()
            .open_info(open)
            .is_ok()
    );
    assert!(engine.pending_module_source().is_none());
    assert!(engine.loader_initialization_owner().is_none());
    engine
        .files
        .as_mut()
        .unwrap()
        .vfs_for_test()
        .close_open(open)
        .unwrap();
    assert_eq!(
        engine.load_library_module(r"C:\app\plugin.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
}
