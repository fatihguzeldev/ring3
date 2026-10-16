use super::*;
use crate::{
    files::{ATTR_DIRECTORY, FileTimes32},
    memory::{PAGE_SIZE, PageRange, Permissions},
    process::{FileConfig32, ModuleOperation32, ModuleRuntimeConfig32, ProcessLaunch32},
    windows::files::FileApi32 as Api,
};
const PATH: u32 = 0x1000;
fn bytes() -> Vec<u8> {
    crate::process::windows::process::test_image_for_startup()
}
fn prepare_main(engine: &mut EngineInstance) {
    engine.register_guest_module("cwd.exe", &bytes()).unwrap();
}
fn start(engine: &mut EngineInstance) {
    assert!(matches!(
        engine.start_module_process("cwd.exe", None, ModuleRuntimeConfig32::default()),
        Ok(ModuleOperation32::Loaded { handle: 0x400000 })
    ));
    engine.map(0x1000, 4, 3).unwrap();
    engine.map(CODE, 1, 7).unwrap();
}
fn read_cwd(engine: &EngineInstance) -> Vec<u16> {
    let descriptor = engine.process_peb_address().unwrap() + 0x124;
    let mut bytes = [0; 8];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(descriptor), &mut bytes)
        .unwrap();
    let length = usize::from(u16::from_le_bytes(bytes[..2].try_into().unwrap()));
    let pointer = u32::from_le_bytes(bytes[4..].try_into().unwrap());
    let mut bytes = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(pointer), &mut bytes)
        .unwrap();
    bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}
fn path(engine: &mut EngineInstance, value: &str, wide: bool) {
    let mut bytes = if wide {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        value.as_bytes().to_vec()
    };
    bytes.extend_from_slice(if wide { &[0, 0] } else { &[0] });
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(PATH), &bytes)
        .unwrap();
}
fn mount(engine: &mut EngineInstance, value: &str) {
    engine
        .mount_directory(
            &value.encode_utf16().collect::<Vec<_>>(),
            ATTR_DIRECTORY,
            FileTimes32::default(),
        )
        .unwrap();
}
fn bare_engine() -> EngineInstance {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    engine.configure_cpu_clock(1_000_000, 0).unwrap();
    engine
}

#[test]
fn unconfigured_launch_adopts_vfs_cwd_and_guest_calls_update_one_canonical_path() {
    let mut engine = bare_engine();
    engine.configure_files(FileConfig32::default()).unwrap();
    mount(&mut engine, "C:\\initial");
    mount(&mut engine, "C:\\target");
    engine.apply_launch_file_directory("C:\\initial").unwrap();
    prepare_main(&mut engine);
    start(&mut engine);
    assert_eq!(
        read_cwd(&engine),
        "C:\\initial".encode_utf16().collect::<Vec<_>>()
    );
    path(&mut engine, "..\\target\\.", true);
    assert_eq!(
        complete(&mut engine, Api::SetCurrentDirectoryW.id(), &[PATH]),
        1
    );
    assert_eq!(
        read_cwd(&engine),
        engine.configured_file_current_directory().unwrap()
    );
    assert_eq!(
        read_cwd(&engine),
        "C:\\target".encode_utf16().collect::<Vec<_>>()
    );
    path(&mut engine, "..\\initial", false);
    assert_eq!(
        complete(&mut engine, Api::SetCurrentDirectoryA.id(), &[PATH]),
        1
    );
    assert_eq!(
        read_cwd(&engine),
        engine.configured_file_current_directory().unwrap()
    );
    assert_eq!(
        read_cwd(&engine),
        "C:\\initial".encode_utf16().collect::<Vec<_>>()
    );
}

#[test]
fn denied_peb_output_keeps_vfs_and_allocator_unchanged_then_retries_and_reuses_storage() {
    let mut engine = bare_engine();
    engine.configure_files(FileConfig32::default()).unwrap();
    mount(&mut engine, "C:\\one");
    mount(&mut engine, "C:\\two");
    prepare_main(&mut engine);
    start(&mut engine);
    path(&mut engine, "C:\\one", true);
    let token = capture(&mut engine, Api::SetCurrentDirectoryW.id(), &[PATH], None);
    let page = PageRange::new(
        GuestAddress(engine.process_peb_address().unwrap() / PAGE_SIZE * PAGE_SIZE),
        1,
    )
    .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ)
        .unwrap();
    let revision = engine.files.as_ref().unwrap().vfs.revision();
    let snapshot = engine
        .process_runtime
        .startup
        .as_ref()
        .unwrap()
        .allocation_snapshot_for_test();
    let before = read_cwd(&engine);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
    assert_eq!(engine.files.as_ref().unwrap().vfs.revision(), revision);
    assert_eq!(engine.configured_file_current_directory().unwrap(), before);
    assert_eq!(read_cwd(&engine), before);
    assert_eq!(
        engine
            .process_runtime
            .startup
            .as_ref()
            .unwrap()
            .allocation_snapshot_for_test(),
        snapshot
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(page, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        complete(&mut engine, Api::SetCurrentDirectoryW.id(), &[PATH]),
        1
    );
    path(&mut engine, "C:\\two", true);
    assert_eq!(
        complete(&mut engine, Api::SetCurrentDirectoryW.id(), &[PATH]),
        1
    );
    let extent = engine
        .process_runtime
        .startup
        .as_ref()
        .unwrap()
        .allocation_extent_for_test();
    for _ in 0..32 {
        for value in ["C:\\two", "C:\\one"] {
            path(&mut engine, value, true);
            assert_eq!(
                complete(&mut engine, Api::SetCurrentDirectoryW.id(), &[PATH]),
                1
            );
            assert_eq!(
                read_cwd(&engine),
                engine.configured_file_current_directory().unwrap()
            );
        }
    }
    assert_eq!(
        engine
            .process_runtime
            .startup
            .as_ref()
            .unwrap()
            .allocation_extent_for_test(),
        extent
    );
}

#[test]
fn explicit_launch_directory_matches_vfs_when_configured_before_or_after_files() {
    for files_first in [false, true] {
        let mut engine = bare_engine();
        if files_first {
            engine.configure_files(FileConfig32::default()).unwrap();
            mount(&mut engine, "C:\\configured");
        }
        engine
            .configure_process_launch(ProcessLaunch32 {
                current_directory: "C:\\configured".into(),
                ..ProcessLaunch32::default()
            })
            .unwrap();
        if !files_first {
            engine.configure_files(FileConfig32::default()).unwrap();
        }
        assert_eq!(
            engine.configured_file_current_directory().unwrap(),
            "C:\\configured".encode_utf16().collect::<Vec<_>>()
        );
        prepare_main(&mut engine);
        start(&mut engine);
        assert_eq!(
            read_cwd(&engine),
            engine.configured_file_current_directory().unwrap()
        );
    }
}
