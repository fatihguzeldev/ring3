use crate::{
    loader::{ModuleError32, normalize_module_identity},
    process::{EngineInstance, HostError, ModuleOperation32, loading::modules::ModulePlanProbe32},
};

fn dll(base: u32) -> Vec<u8> {
    let mut bytes = crate::process::windows::process::test_image_for_startup();
    bytes[0x96..0x98].copy_from_slice(&0x2103u16.to_le_bytes());
    bytes[0xa8..0xac].fill(0);
    bytes[0xb4..0xb8].copy_from_slice(&base.to_le_bytes());
    bytes
}

#[test]
fn qualified_sources_keep_distinct_dll_identity_and_exact_peb_file_names() {
    let mut engine = EngineInstance::new(256, 7).unwrap();
    engine.register_kernel32_module(0x70000000).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    engine
        .register_guest_module(r"C:\one\Plugin.dll", &dll(0x600000))
        .unwrap();
    engine
        .register_guest_module(r"C:\two\Plugin.dll", &dll(0x800000))
        .unwrap();
    engine
        .start_module_process("main.exe", None, Default::default())
        .unwrap();
    assert_eq!(
        engine
            .load_library_module("C:/one/PLUGIN.DLL", None)
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    assert_eq!(
        engine
            .load_library_module(r"C:\two\plugin.dll", None)
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x800000 }
    );
    assert_eq!(
        engine.module_handle(Some(r"C:\one\plugin.dll")).unwrap(),
        Some(0x600000)
    );
    assert_eq!(
        engine.module_handle(Some(r"C:\two\plugin.dll")).unwrap(),
        Some(0x800000)
    );
    assert_eq!(
        engine.virtual_module_file_name(0x600000).unwrap(),
        r"C:\one\Plugin.dll"
    );
    assert_eq!(
        engine.virtual_module_file_name(0x800000).unwrap(),
        r"C:\two\Plugin.dll"
    );
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.load_library_module(r"C:\third\plugin.dll", None),
        Err(HostError::Module(ModuleError32::MissingModule))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    engine.free_library_module(0x600000).unwrap();
    assert_eq!(
        engine.module_handle(Some(r"C:\one\plugin.dll")).unwrap(),
        None
    );
    assert_eq!(
        engine.module_handle(Some(r"C:\two\plugin.dll")).unwrap(),
        Some(0x800000)
    );
}

#[test]
fn source_probe_returns_exact_unresolved_identity_without_installing_ram() {
    let engine = EngineInstance::new(32, 7).unwrap();
    let paths = engine.capture_module_paths(None).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let identity = normalize_module_identity(r"C:\specific\missing.dll").unwrap();
    let ModulePlanProbe32::Missing(missing) = engine
        .probe_captured_module_load(&identity, None, None, &paths)
        .unwrap()
    else {
        panic!("missing source")
    };
    assert_eq!(missing, identity);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
}
