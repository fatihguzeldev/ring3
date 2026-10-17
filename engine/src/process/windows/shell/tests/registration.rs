use super::*;
use crate::loader::{ImportSymbol32, ModuleError32, ModuleLimits32};
#[path = "../../../../../tests/loader/modules/linking/native/image.rs"]
mod pe;

#[test]
fn shell_registration_links_historical_folder_imports_to_the_same_canonical_gates() {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.register_shell32_module(0x76000000).unwrap();
    assert!(engine.has_registered_module("shfolder.dll"));
    let image = pe::image(
        false,
        0x400000,
        &[pe::Import {
            module: "shfolder.dll",
            symbols: vec![
                pe::Symbol::Name("SHGetFolderPathA"),
                pe::Symbol::Name("SHGetFolderPathW"),
            ],
        }],
        &[],
    );
    engine.register_guest_module("folders.exe", &image).unwrap();
    engine
        .start_module_process("folders.exe", None, Default::default())
        .unwrap();
    let alias = engine.module_handle(Some("shfolder.dll")).unwrap().unwrap();
    let shell = engine.module_handle(Some("shell32.dll")).unwrap().unwrap();
    for api in [Api::SHGetFolderPathA, Api::SHGetFolderPathW] {
        let symbol = ImportSymbol32::Name(api.name().into());
        let forwarded = engine.module_export(alias, &symbol).unwrap();
        let canonical = engine.module_export(shell, &symbol).unwrap();
        assert_eq!(forwarded, canonical);
        assert_eq!(
            engine
                .module_gates()
                .unwrap()
                .iter()
                .find(|gate| gate.entry == forwarded.address())
                .unwrap()
                .id,
            api.id()
        );
    }
}

#[test]
fn shell_and_folder_facade_admission_is_atomic_when_only_one_module_slot_remains() {
    let mut engine = engine();
    engine
        .configure_module_limits(ModuleLimits32 {
            modules: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        engine.register_shell32_module(0x76000000),
        Err(HostError::Module(ModuleError32::Capacity))
    );
    assert!(!engine.has_registered_module("shell32.dll"));
    assert!(!engine.has_registered_module("shfolder.dll"));
}
