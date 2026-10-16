use super::*;

#[test]
fn altered_search_datafile_keeps_shared_resource_layout_without_resolving_imports() {
    for flags in [2, 10] {
        let bytes = image(true, 0x600000, Some("missing-dependency.dll"), None);
        let assets = [(r"C:\app\resources.dll", bytes.as_slice())];
        let mut engine = filesystem(&assets);
        engine
            .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
            .unwrap();
        engine
            .start_module_process(r"C:\app\main.exe", None, Default::default())
            .unwrap();
        let operation = engine
            .load_library_module_flags(r"C:\app\resources.dll", flags)
            .unwrap();
        let (handle, waits) = finish(&mut engine, operation, &assets);
        assert_eq!(waits, 1);
        assert_eq!(handle & 3, 1);
        assert_eq!(
            engine.module_handle(Some(r"C:\app\resources.dll")).unwrap(),
            None
        );
        assert_eq!(engine.resource_only_modules().count(), 1);
        assert_eq!(
            engine
                .module_handle(Some("missing-dependency.dll"))
                .unwrap(),
            None
        );
        let mut signature = [0; 2];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(handle & !3), &mut signature)
            .unwrap();
        assert_eq!(&signature, b"MZ");
        assert_eq!(
            engine.free_resource_only_module(handle).unwrap(),
            ModuleOperation32::Released
        );
        assert_eq!(engine.resource_only_modules().count(), 0);
        assert!(engine.pending_module_source().is_none());
        assert!(engine.pending_file_request().unwrap().is_none());
        assert!(engine.loader_initialization_owner().is_none());
    }
}
