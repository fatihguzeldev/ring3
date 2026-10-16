use super::*;
use crate::loader::normalize_module_identity;

fn selected() -> EngineInstance {
    let mut engine = filesystem(&[]);
    engine
        .mount_directory(
            &r"C:\work".encode_utf16().collect::<Vec<_>>(),
            ATTR_DIRECTORY,
            FileTimes32::default(),
        )
        .unwrap();
    for (name, base, dll) in [
        (r"C:\app\main.exe", 0x400000, false),
        (r"C:\plugins\pick.dll", 0x600000, true),
        (r"C:\work\pick.dll", 0x800000, true),
    ] {
        engine
            .register_guest_module(name, &image(dll, base, None, None))
            .unwrap();
    }
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    engine.apply_launch_file_directory(r"C:\work").unwrap();
    engine
}

#[test]
fn dll_directory_policy_distinguishes_default_empty_and_named_search() {
    for (directory, expected, handle) in [
        (None, r"C:\work\pick.dll", Some(0x800000)),
        (Some(""), r"C:\app\pick.dll", None),
        (Some(r"C:\plugins"), r"C:\plugins\pick.dll", Some(0x600000)),
    ] {
        let mut engine = selected();
        engine.set_module_dll_directory(directory.map(|path| path.encode_utf16().collect()));
        assert_eq!(
            engine.resolve_module_source_name("pick.dll").unwrap(),
            normalize_module_identity(expected).unwrap()
        );
        if let Some(handle) = handle {
            assert_eq!(
                engine.load_library_module("pick.dll", None),
                Ok(ModuleOperation32::Loaded { handle })
            );
        }
    }
    let mut engine = selected();
    engine
        .register_guest_module(r"C:\app\pick.dll", &image(true, 0xa00000, None, None))
        .unwrap();
    engine.set_module_dll_directory(Some(r"C:\plugins".encode_utf16().collect()));
    assert_eq!(
        engine.load_library_module("pick.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0xa00000 })
    );
}

#[test]
fn a_load_captures_its_directory_policy_before_later_process_changes() {
    let mut engine = selected();
    engine.set_module_dll_directory(Some(r"C:\plugins".encode_utf16().collect()));
    let captured = engine.capture_module_paths(None).unwrap();
    engine.set_module_dll_directory(None);
    assert_eq!(
        engine
            .resolve_module_source_captured("pick.dll", &captured)
            .unwrap(),
        normalize_module_identity(r"C:\plugins\pick.dll").unwrap()
    );
    assert_eq!(
        engine.resolve_module_source_name("pick.dll").unwrap(),
        normalize_module_identity(r"C:\work\pick.dll").unwrap()
    );
    assert_eq!(
        engine.load_library_module("pick.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x800000 })
    );
    assert_eq!(
        engine.module_handle(Some(r"C:\plugins\pick.dll")).unwrap(),
        None
    );
}
