use super::*;

#[test]
fn ordinary_full_path_library_dependencies_use_the_executable_directory() {
    let mut engine = filesystem(&[]);
    for (name, bytes) in [
        (r"C:\app\main.exe", image(false, 0x400000, None, None)),
        (
            r"C:\plugins\outer.dll",
            image(true, 0x600000, Some("dependency.dll"), None),
        ),
        (r"C:\app\dependency.dll", image(true, 0x800000, None, None)),
        (
            r"C:\plugins\dependency.dll",
            image(true, 0xa00000, None, None),
        ),
    ] {
        engine.register_guest_module(name, &bytes).unwrap();
    }
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
        .unwrap();
    assert_eq!(
        engine.load_library_module(r"C:\plugins\outer.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\dependency.dll"))
            .unwrap(),
        Some(0x800000)
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\plugins\dependency.dll"))
            .unwrap(),
        None
    );
    let mut iat = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x602400), &mut iat)
        .unwrap();
    assert_eq!(u32::from_le_bytes(iat), 0x801000);
}

#[test]
fn slash_qualified_executable_captures_its_normalized_application_directory() {
    let mut engine = filesystem(&[]);
    engine
        .register_guest_module(
            r"C:\app\main.exe",
            &image(false, 0x400000, Some("dependency.dll"), None),
        )
        .unwrap();
    engine
        .register_guest_module(r"C:\app\dependency.dll", &image(true, 0x600000, None, None))
        .unwrap();
    assert_eq!(
        engine.start_module_process("C:/app/main.exe", None, Default::default()),
        Ok(ModuleOperation32::Loaded { handle: 0x400000 })
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\dependency.dll"))
            .unwrap(),
        Some(0x600000)
    );
}
