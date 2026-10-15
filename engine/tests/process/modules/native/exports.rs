use super::fixtures::{DLL_BASE, EXE_BASE, forwarding_exe, linked_image};
use ring3_engine::{
    loader::{ImportSymbol32, LoadError, ModuleError32},
    process::{EngineInstance, HostError, ModuleOperation32, ModuleRuntimeConfig32},
    cpu::x86::SegmentRegister,
};

#[test]
fn direct_lookup_uses_live_eat_without_requiring_a_callback_context() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.register_guest_module("main.exe", &linked_image(EXE_BASE, false, &[])).unwrap();
    engine.register_guest_module("helper.dll", &linked_image(DLL_BASE, true, &[])).unwrap();
    engine.start_module_process("main.exe", None, ModuleRuntimeConfig32::default()).unwrap();
    engine.load_library_module("helper.dll", None).unwrap();
    engine.write32(DLL_BASE + 0x3030, 0x1008).unwrap();
    engine.set_segment_base(SegmentRegister::Ss, 0x1000).unwrap();
    let before = engine.arena().to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    let symbol = ImportSymbol32::Name("answer".into());
    assert_eq!(engine.resolve_module_export(DLL_BASE, &symbol).unwrap(),
        ModuleOperation32::ExportResolved { address: DLL_BASE + 0x1008 });
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);

    engine.write32(DLL_BASE + 0x3030, 0x7000).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(engine.resolve_module_export(DLL_BASE, &symbol),
        Err(HostError::Module(ModuleError32::Pe(LoadError::Malformed))));
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
}

#[test]
fn a_forwarder_to_an_already_loaded_target_adopts_its_dependency() {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.register_guest_module("main.exe", &forwarding_exe(EXE_BASE, "target.answer")).unwrap();
    engine.register_guest_module("target.dll", &linked_image(DLL_BASE, true, &[])).unwrap();
    engine.start_module_process("main.exe", None, ModuleRuntimeConfig32::default()).unwrap();
    engine.load_library_module("target.dll", None).unwrap();
    assert_eq!(engine.resolve_module_export(EXE_BASE, &ImportSymbol32::Name("answer".into())).unwrap(),
        ModuleOperation32::ExportResolved { address: DLL_BASE + 0x1000 });
    assert_eq!(engine.free_library_module(DLL_BASE).unwrap(), ModuleOperation32::Released);
    assert_eq!(engine.module_handle(Some("target.dll")).unwrap(), Some(DLL_BASE),
        "the pinned forwarding root retains its target after its explicit reference is released");
}
