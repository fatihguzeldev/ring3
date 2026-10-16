use super::*;
use crate::{
    loader::SystemModule32,
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::process::startup::loader::tests::{image, name, word},
    process::{FileConfig32, FileEnvironment32, ModuleOperation32, ModuleRuntimeConfig32},
};
const BUFFER: u32 = 0x30000;
pub(super) fn engine() -> (EngineInstance, u32) {
    engine_with_profile(Default::default())
}
pub(super) fn engine_with_profile(
    profile: crate::windows::nls::NlsProfile32,
) -> (EngineInstance, u32) {
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine.configure_nls(profile).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    engine.register_guest_module("main.exe", &image()).unwrap();
    engine
        .register_system_module(SystemModule32 {
            name: "first.dll".into(),
            preferred_base: 0x500000,
            exports: Vec::new(),
        })
        .unwrap();
    for (name, base) in [("ordinary.dll", 0x800000u32), ("only.dll", 0x900000)] {
        let mut bytes = image();
        bytes[0x96..0x98].copy_from_slice(&0x2102u16.to_le_bytes());
        bytes[0xa8..0xac].copy_from_slice(&0u32.to_le_bytes());
        bytes[0xb4..0xb8].copy_from_slice(&base.to_le_bytes());
        engine.register_guest_module(name, &bytes).unwrap();
    }
    assert!(matches!(
        engine.start_module_process("main.exe", None, ModuleRuntimeConfig32::default()),
        Ok(ModuleOperation32::Loaded { handle: 0x400000 })
    ));
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(BUFFER), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert!(matches!(
        engine.load_library_module("first.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x500000 })
    ));
    assert!(matches!(
        engine.load_library_module("ordinary.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x800000 })
    ));
    let ModuleOperation32::Loaded { handle } =
        engine.load_resource_only_module("only.dll", 2).unwrap()
    else {
        panic!("resource-only image must be directly loaded");
    };
    (engine, handle)
}
fn written(engine: &EngineInstance, length: usize, wide: bool) -> Vec<u16> {
    let mut bytes = vec![0; length * if wide { 2 } else { 1 }];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(BUFFER), &mut bytes)
        .unwrap();
    if wide {
        bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    } else {
        bytes.into_iter().map(u16::from).collect()
    }
}

#[test]
fn filename_api_matches_stable_peb_full_names_for_guest_and_system_dlls() {
    let (mut engine, _) = engine();
    for (handle, expected) in [
        (0x500000, "C:\\Windows\\System32\\first.dll"),
        (0x800000, "C:\\game\\ordinary.dll"),
    ] {
        for api in [
            ProcessApi32::GetModuleFileNameA,
            ProcessApi32::GetModuleFileNameW,
        ] {
            let reply = engine
                .execute_process_call(api, &[handle, BUFFER, 256])
                .unwrap();
            assert!(reply.error.is_none());
            assert_eq!(
                written(&engine, reply.value as usize, api.name().ends_with('W')),
                expected.encode_utf16().collect::<Vec<_>>()
            );
        }
        let head = engine.process_peb_address().unwrap() + 0x20c;
        let mut entry = word(engine.memory().unwrap(), head);
        for _ in 0..16 {
            if entry == head || word(engine.memory().unwrap(), entry + 24) == handle {
                break;
            }
            entry = word(engine.memory().unwrap(), entry);
        }
        assert_ne!(entry, head);
        let epoch = engine.files_epoch().unwrap();
        engine
            .set_files_environment(
                epoch,
                FileEnvironment32 {
                    system_directory: "C:\\Temp".encode_utf16().collect(),
                    ..FileEnvironment32::default()
                },
            )
            .unwrap();
        assert_eq!(engine.virtual_module_file_name(handle).unwrap(), expected);
        assert_eq!(
            name(engine.memory().unwrap(), entry + 36),
            expected.encode_utf16().collect::<Vec<_>>()
        );
    }
}

#[test]
fn resource_only_and_unknown_handles_fail_without_touching_filename_output() {
    let (mut engine, handle) = engine();
    for handle in [handle, 0xdead0000] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(BUFFER), &[0xcc; 16])
            .unwrap();
        let version = engine.memory().unwrap().transaction_version();
        assert!(matches!(
            engine.execute_process_call(ProcessApi32::GetModuleFileNameW, &[handle, BUFFER, 8]),
            Err(HostError::WindowsProcess(ProcessError32::InvalidHandle))
        ));
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(written(&engine, 8, false), vec![0xcc; 8]);
    }
    assert_eq!(ProcessError32::InvalidHandle.win32(), 6);
    assert!(matches!(
        engine.execute_process_call(
            ProcessApi32::GetModuleFileNameW,
            &[0x500000, 0xfffffff0, 256]
        ),
        Err(HostError::WindowsProcess(ProcessError32::NoAccess))
    ));
}
