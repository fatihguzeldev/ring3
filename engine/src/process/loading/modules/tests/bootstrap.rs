use crate::{
    loader::{ImportSymbol32, ModuleError32, ModuleLimits32, ResolvedExport32},
    memory::{Access, GuestAddress, MemoryError},
    process::{EngineInstance, HostError, ModuleOperation32},
    windows::exceptions::ExceptionApi32,
};

fn engine(pages: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(pages, 7).unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    engine
}

fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

fn loader_bases(engine: &EngineInstance, list: u32) -> Vec<u32> {
    let peb = engine.process_peb_address().unwrap();
    let head = word(engine, peb + 0x0c) + 0x0c + list * 8;
    let mut link = word(engine, head);
    let mut previous = head;
    let mut bases = Vec::new();
    while link != head {
        assert!(bases.len() < 8);
        assert_eq!(word(engine, link + 4), previous);
        bases.push(word(engine, link - list * 8 + 24));
        previous = link;
        link = word(engine, link);
    }
    assert_eq!(word(engine, head + 4), previous);
    bases
}

#[test]
fn no_import_windows_process_bootstraps_real_ntdll_in_the_same_plan_and_peb_lists() {
    let mut engine = engine(32);
    engine.register_kernel32_module(0x70000000).unwrap();
    assert_eq!(engine.module_handle(Some("ntdll.dll")).unwrap(), None);
    assert_eq!(
        engine
            .start_module_process("main.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    );
    let ntdll = engine.module_handle(Some("NTDLL.DLL")).unwrap().unwrap();
    assert_eq!(ntdll, 0x77f00000);
    assert_eq!(engine.module_handle(Some("kernel32.dll")).unwrap(), None);
    assert_eq!(engine.loaded_windows_modules().len(), 2);
    assert_eq!(loader_bases(&engine, 0), [0x400000, ntdll]);
    assert_eq!(loader_bases(&engine, 1), [0x400000, ntdll]);
    assert_eq!(loader_bases(&engine, 2), [ntdll]);
    let ResolvedExport32::SystemGate { gate, .. } = engine
        .module_export(ntdll, &ImportSymbol32::Name("NtContinue".into()))
        .unwrap()
    else {
        panic!("native runtime entry must be a mapped address");
    };
    engine
        .memory()
        .unwrap()
        .check_span(gate.entry, 1, Access::Execute)
        .unwrap();
    assert_eq!(
        engine.module_gate_at(gate.entry.0).unwrap().id,
        ExceptionApi32::NtContinue.id()
    );
    assert!(matches!(
        engine.free_library_module(ntdll),
        Err(HostError::Module(ModuleError32::InvalidHandle))
    ));
    assert_eq!(
        engine.module_handle(Some("ntdll.dll")).unwrap(),
        Some(ntdll)
    );
}

#[test]
fn native_runtime_page_capacity_failure_leaves_startup_and_vm_unpublished() {
    let mut engine = engine(2);
    engine.register_kernel32_module(0x70000000).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    assert!(matches!(
        engine.start_module_process("main.exe", None, Default::default()),
        Err(HostError::Module(ModuleError32::Memory(
            MemoryError::Capacity
        )))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.loaded_windows_modules().len(), 0);
    assert_eq!(engine.process_peb_address(), None);
    assert_eq!(engine.module_teb_address(), None);
    assert!(engine.windows_memory.is_empty());
    assert_eq!(engine.loader_initialization_owner(), None);
}

#[test]
fn failed_atomic_windows_registration_does_not_enable_runtime_bootstrap() {
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine
        .configure_module_limits(ModuleLimits32 {
            modules: 2,
            ..Default::default()
        })
        .unwrap();
    engine
        .register_guest_module(
            "main.exe",
            &crate::process::windows::process::test_image_for_startup(),
        )
        .unwrap();
    assert!(matches!(
        engine.register_kernel32_module(0x70000000),
        Err(HostError::Module(ModuleError32::Capacity))
    ));
    assert!(!engine.has_registered_module("kernel32.dll"));
    assert!(!engine.has_registered_module("ntdll.dll"));
    assert!(!engine.modules.windows_runtime);
    assert_eq!(
        engine
            .start_module_process("main.exe", None, Default::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    );
    assert_eq!(engine.loaded_windows_modules().len(), 1);
    assert_eq!(engine.module_handle(Some("ntdll.dll")).unwrap(), None);
}
