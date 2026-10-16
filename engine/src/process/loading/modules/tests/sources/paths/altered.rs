use super::*;

#[test]
fn altered_search_path_uses_the_loaded_library_directory_before_the_executable_directory() {
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
        engine.load_library_module_flags(r"C:\plugins\outer.dll", 8),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\plugins\dependency.dll"))
            .unwrap(),
        Some(0xa00000)
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\dependency.dll"))
            .unwrap(),
        None
    );
    let mut iat = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x602400), &mut iat)
        .unwrap();
    assert_eq!(u32::from_le_bytes(iat), 0xa01000);
}
