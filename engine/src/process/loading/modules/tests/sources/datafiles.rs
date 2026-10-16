use super::*;
use crate::files::FileError32;

#[test]
fn lazy_resource_only_sources_use_the_same_wait_owner_and_keep_tagged_mapping_identity() {
    let mut bytes = image(true, 0x600000, None, None);
    bytes[0x201] = 0x5a;
    let assets = [(r"C:\app\resources.dll", bytes.as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    let first = engine
        .load_resource_only_module(r"C:\app\resources.dll", 2)
        .unwrap();
    assert!(matches!(first, ModuleOperation32::NeedBytes { .. }));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let (handle, waits) = finish(&mut engine, first, &assets);
    assert_eq!(waits, 1);
    assert_eq!(handle & 3, 1);
    assert_eq!(
        engine.module_handle(Some(r"C:\app\resources.dll")).unwrap(),
        None
    );
    let mut byte = [0];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress((handle & !3) + 0x201), &mut byte)
        .unwrap();
    assert_eq!(byte, [0x5a]);
    engine.free_resource_only_module(handle).unwrap();
    assert!(engine.pending_module_source().is_none());
    assert!(engine.loader_initialization_owner().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn resource_only_reload_refreshes_changed_vfs_bytes_without_changing_existing_mapping() {
    let mut old = image(true, 0x600000, None, None);
    old[0x201] = 0x11;
    let assets = [(r"C:\app\resources.dll", old.as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let first = engine
        .load_library_module(r"C:\app\resources.dll", None)
        .unwrap();
    finish(&mut engine, first, &assets);
    engine.free_library_module(0x600000).unwrap();
    let ModuleOperation32::Loaded { handle: retained } = engine
        .load_resource_only_module(r"C:\app\resources.dll", 2)
        .unwrap()
    else {
        panic!("cached resource")
    };
    let mut new = old.clone();
    new[0x201] = 0x22;
    engine
        .replace_provider_file(
            &r"C:\app\resources.dll".encode_utf16().collect::<Vec<_>>(),
            &new,
        )
        .unwrap();
    let ModuleOperation32::Loaded { handle: current } = engine
        .load_resource_only_module(r"C:\app\resources.dll", 0x20)
        .unwrap()
    else {
        panic!("refreshed overlay")
    };
    assert_eq!(current & 3, 2);
    assert_ne!(current & !3, retained & !3);
    let mut byte = [0];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress((retained & !3) + 0x201), &mut byte)
        .unwrap();
    assert_eq!(byte, [0x11]);
    engine
        .memory()
        .unwrap()
        .read(GuestAddress((current & !3) + 0x1001), &mut byte)
        .unwrap();
    assert_eq!(byte, [0x22]);
    engine.free_resource_only_module(retained).unwrap();
    engine.free_resource_only_module(current).unwrap();
}

#[test]
fn lazy_resource_byte_wait_retains_its_read_grant_and_blocks_content_replacement() {
    let mut bytes = image(true, 0x600000, None, None);
    bytes[0x201] = 0x31;
    let assets = [(r"C:\app\resources.dll", bytes.as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let first = engine
        .load_resource_only_module(r"C:\app\resources.dll", 2)
        .unwrap();
    let ModuleOperation32::NeedBytes { request } = first else {
        panic!("resource source wait")
    };
    let path = r"C:\app\resources.dll".encode_utf16().collect::<Vec<_>>();
    let version = engine.provider_path_version(&path).unwrap();
    let mut replacement = bytes.clone();
    replacement[0x201] = 0x72;
    assert_eq!(
        engine.replace_provider_file(&path, &replacement),
        Err(HostError::File(FileError32::SharingViolation))
    );
    assert_eq!(engine.provider_path_version(&path).unwrap(), version);
    assert_eq!(
        engine.pending_module_source().unwrap().request,
        Some(request)
    );
    let (handle, waits) = finish(&mut engine, first, &assets);
    assert_eq!(waits, 1);
    assert_eq!(
        engine.replace_provider_file(&path, &replacement).unwrap(),
        replacement.len()
    );
    assert_ne!(engine.provider_path_version(&path).unwrap(), version);
    let mut byte = [0];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress((handle & !3) + 0x201), &mut byte)
        .unwrap();
    assert_eq!(byte, [0x31]);
    engine.free_resource_only_module(handle).unwrap();
}

#[test]
fn malformed_lazy_resource_source_closes_its_lease_without_publishing_mapping() {
    let assets = [(r"C:\app\resources.dll", b"not a PE image".as_slice())];
    let mut engine = filesystem(&assets);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let pages = engine.memory().unwrap().mapped_pages();
    let modules = engine.loaded_windows_modules().len();
    let ModuleOperation32::NeedBytes { request } = engine
        .load_resource_only_module(r"C:\app\resources.dll", 0x20)
        .unwrap()
    else {
        panic!("malformed lazy resource")
    };
    let pending = engine.pending_module_source().unwrap();
    supply(&mut engine, request, &assets);
    assert!(matches!(
        engine.resume_module_source(pending.epoch, pending.token),
        Err(HostError::Module(crate::loader::ModuleError32::Pe(_)))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.loaded_windows_modules().len(), modules);
    assert!(engine.resource_only_modules().next().is_none());
    assert!(engine.pending_module_source().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    assert!(engine.loader_initialization_owner().is_none());
    let lease = engine
        .open_provider_file(
            &r"C:\app\resources.dll".encode_utf16().collect::<Vec<_>>(),
            true,
        )
        .unwrap();
    engine.close_provider_file(lease).unwrap();
}
