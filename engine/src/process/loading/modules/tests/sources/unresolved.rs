use super::*;
use crate::{loader::ImportSymbol32, process::loading::modules::ModuleRequest32};

fn started() -> EngineInstance {
    let mut engine = filesystem(&[]);
    engine
        .register_guest_module(r"C:\app\main.exe", &image(false, 0x400000, None, None))
        .unwrap();
    engine
        .start_module_process(r"C:\app\main.exe", None, Default::default())
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

#[test]
fn ordinary_executable_library_load_does_not_bind_its_missing_imports() {
    let mut engine = started();
    let bytes = image(false, 0x600000, Some("absent.dll"), None);
    let original_iat = u32::from_le_bytes(bytes[0x800..0x804].try_into().unwrap());
    engine
        .register_guest_module(r"C:\app\other.exe", &bytes)
        .unwrap();
    assert_eq!(
        engine.load_library_module(r"C:\app\other.exe", None),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
    assert_eq!(
        engine.virtual_module_file_name_units(0x600000).unwrap(),
        r"C:\app\other.exe".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(word(&engine, 0x602400), original_iat);
    assert_eq!(engine.module_handle(Some("absent.dll")).unwrap(), None);
    assert_eq!(
        engine.module_handle(Some(r"C:\app\other.exe")).unwrap(),
        Some(0x600000)
    );
    assert_eq!(
        engine
            .resolve_module_export(0x600000, &ImportSymbol32::Name("Run".into()))
            .unwrap(),
        ModuleOperation32::ExportResolved { address: 0x601000 }
    );
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(
        engine.module_handle(Some(r"C:\app\other.exe")).unwrap(),
        None
    );
}

#[test]
fn dont_resolve_library_keeps_original_iat_tls_and_suppresses_all_notifications() {
    let mut engine = started();
    let mut bytes = image(true, 0x600000, Some("absent.dll"), None);
    bytes[0xa8..0xac].copy_from_slice(&0x1000u32.to_le_bytes());
    bytes[0x140..0x144].copy_from_slice(&0x2e00u32.to_le_bytes());
    bytes[0x144..0x148].copy_from_slice(&24u32.to_le_bytes());
    for (offset, value) in [
        (0, 0x602e60u32),
        (4, 0x602e64),
        (8, 0x602e40),
        (12, 0x602e80),
        (16, 16),
    ] {
        bytes[0x1200 + offset..0x1204 + offset].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x1240..0x1244].copy_from_slice(&0xabcdef01u32.to_le_bytes());
    bytes[0x1280..0x1284].copy_from_slice(&0x601000u32.to_le_bytes());
    let original_iat = u32::from_le_bytes(bytes[0x800..0x804].try_into().unwrap());
    engine
        .register_guest_module(r"C:\app\unresolved.dll", &bytes)
        .unwrap();
    assert_eq!(
        engine.execute_module_request(&ModuleRequest32::Load {
            name: r"C:\app\unresolved.dll".into(),
            flags: 1
        }),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
    assert_eq!(word(&engine, 0x602400), original_iat);
    assert_eq!(word(&engine, 0x602e40), 0xabcdef01);
    let head = word(&engine, engine.process_peb_address().unwrap() + 0x0c) + 0x1c;
    let mut entry = word(&engine, head);
    let mut count = 0;
    while entry != head {
        assert_ne!(word(&engine, entry + 8), 0x600000);
        entry = word(&engine, entry);
        count += 1;
        assert!(count <= engine.loaded_windows_modules().len());
    }
    assert!(engine.module_initializer().unwrap().is_none());
    assert!(
        engine
            .windows_thread_initializers(2)
            .unwrap()
            .iter()
            .all(|(entry, _)| entry.0 != 0x601000)
    );
    assert_eq!(
        engine.load_library_module(r"C:\app\unresolved.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x600000 })
    );
    engine.free_library_module(0x600000).unwrap();
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\unresolved.dll"))
            .unwrap(),
        Some(0x600000)
    );
    assert_eq!(
        engine.free_library_module(0x600000).unwrap(),
        ModuleOperation32::Released
    );
    assert_eq!(
        engine
            .module_handle(Some(r"C:\app\unresolved.dll"))
            .unwrap(),
        None
    );
    assert!(engine.module_initializer().unwrap().is_none());
}
