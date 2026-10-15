use super::fixtures::{EXE_BASE, linked_image};
use ring3_engine::process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32};

fn catalog(images: &[(&str, u32, &[&str])]) -> EngineInstance {
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine
        .register_guest_module("main.exe", &linked_image(EXE_BASE, false, &[]))
        .unwrap();
    for (name, base, dependencies) in images {
        engine
            .register_guest_module(name, &linked_image(*base, true, dependencies))
            .unwrap();
    }
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: EXE_BASE }
    );
    engine
}

#[test]
fn a_shared_dependency_survives_until_the_last_explicit_root_is_released() {
    let mut engine = catalog(&[
        ("left.dll", 0x600000, &["shared.dll"]),
        ("right.dll", 0x800000, &["shared.dll"]),
        ("shared.dll", 0xa00000, &[]),
    ]);
    let base_pages = engine.memory().unwrap().mapped_pages();
    assert_eq!(
        engine.load_library_module("left.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    assert_eq!(
        engine.load_library_module("right.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x800000 }
    );
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("left.dll")).unwrap(), None);
    assert_eq!(
        engine.module_handle(Some("shared.dll")).unwrap(),
        Some(0xa00000)
    );
    assert_eq!(
        engine.free_library_module(0x800000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("right.dll")).unwrap(), None);
    assert_eq!(engine.module_handle(Some("shared.dll")).unwrap(), None);
    assert_eq!(engine.memory().unwrap().mapped_pages(), base_pages);
}

#[test]
fn an_unreachable_dependency_cycle_unloads_as_one_closure() {
    let mut engine = catalog(&[
        ("a.dll", 0x600000, &["b.dll"]),
        ("b.dll", 0x800000, &["a.dll"]),
    ]);
    let base_pages = engine.memory().unwrap().mapped_pages();
    assert_eq!(
        engine.load_library_module("a.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(engine.module_handle(Some("a.dll")).unwrap(), None);
    assert_eq!(engine.module_handle(Some("b.dll")).unwrap(), None);
    assert_eq!(engine.memory().unwrap().mapped_pages(), base_pages);
}
