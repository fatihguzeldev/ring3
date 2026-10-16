use super::support::*;
use crate::{
    abi::x86::{STATE_SIZE, decode_state},
    process::{CallError, HostError, ModuleOperation32},
    windows::{WindowsApi32, user32::User32Api32},
};

#[test]
fn window_callback_loads_and_frees_a_dll_while_initializer_can_run_a_nested_provider_callback() {
    let (mut engine, atom) = engine();
    let outer = begin_create(&mut engine, atom, &[WindowsApi32::LoadLibraryW.id()]);
    let outer_generation = engine.generation;
    assert!(engine.pending_call.is_none());
    let mut hwnd = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(crate::memory::GuestAddress(outer.entry_esp + 4), &mut hwnd)
        .unwrap();
    let hwnd = u32::from_le_bytes(hwnd);
    load(&mut engine);
    let init = engine.module_initializer().unwrap().unwrap();
    assert!(engine.callback.is_none() && engine.windows_replies.is_empty());
    assert_eq!(engine.modules.parked_callback_depth(), 1);
    loader_material(&mut engine, init, true);
    assert_ne!(engine.generation, outer_generation);
    call(
        &mut engine,
        User32Api32::SendMessageW as u32,
        &[hwnd, 0x400, 7, 8],
    );
    let nested = engine
        .begin_windows_callback(KEY, engine.generation, RETURN_PC, RETURN_ID)
        .unwrap();
    finish_callback(&mut engine, nested, 42);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        42
    );
    assert_eq!(
        return_initializer(&mut engine, init, 1),
        ModuleOperation32::Loaded { handle: DLL }
    );
    assert_eq!(engine.generation, outer_generation);
    assert_eq!(engine.callback.as_ref().unwrap().token(), outer.token);
    assert_eq!(engine.windows_replies.len(), 1);
    let window = finish_creation(&mut engine, outer);
    material(
        &mut engine,
        &[
            User32Api32::SendMessageW as u32,
            WindowsApi32::FreeLibrary.id(),
        ],
    );
    call(
        &mut engine,
        User32Api32::SendMessageW as u32,
        &[window, 0x401, 0, 0],
    );
    let outer = engine
        .begin_windows_callback(KEY, engine.generation, RETURN_PC, RETURN_ID)
        .unwrap();
    call(&mut engine, WindowsApi32::FreeLibrary.id(), &[DLL]);
    let init = engine.module_initializer().unwrap().unwrap();
    assert_eq!(init.reason, 0);
    loader_material(&mut engine, init, false);
    assert_eq!(
        return_initializer(&mut engine, init, 1),
        ModuleOperation32::Released
    );
    assert_eq!(engine.callback.as_ref().unwrap().token(), outer.token);
    assert_eq!(engine.module_handle(Some("helper.dll")).unwrap(), None);
    finish_callback(&mut engine, outer, 9);
    assert!(engine.pending_call.is_none() && engine.callback.is_none());
}

#[test]
fn rejected_host_admission_and_failed_dll_attach_preserve_the_exact_outer_callback() {
    let (mut engine, atom) = engine();
    let outer = begin_create(&mut engine, atom, &[WindowsApi32::LoadLibraryW.id()]);
    let arena = engine.arena().to_vec();
    let pages = engine.memory().unwrap().mapped_pages();
    assert!(matches!(
        engine.load_library_module("helper.dll", None),
        Err(HostError::Call(CallError::Busy))
    ));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.callback.as_ref().unwrap().token(), outer.token);
    assert_eq!(engine.windows_replies.len(), 1);
    load(&mut engine);
    let attach = engine.module_initializer().unwrap().unwrap();
    loader_material(&mut engine, attach, false);
    let ModuleOperation32::Pending(detach) = return_initializer(&mut engine, attach, 0) else {
        panic!("attach failure must perform real detach")
    };
    assert_eq!(detach.reason, 0);
    loader_material(&mut engine, detach, false);
    assert_eq!(
        return_initializer(&mut engine, detach, 1),
        ModuleOperation32::InitializationFailed
    );
    assert_eq!(engine.callback.as_ref().unwrap().token(), outer.token);
    assert_eq!(engine.windows_replies.len(), 1);
    assert_eq!(engine.module_handle(Some("helper.dll")).unwrap(), None);
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE])
            .unwrap()
            .registers[0],
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 1114);
    assert!(engine.finish_module_initializer(detach.token).is_err());
    assert!(finish_creation(&mut engine, outer) != 0);
}

#[test]
fn process_exit_from_window_callback_discards_outer_only_after_real_detach_completes() {
    let (mut engine, atom) = engine();
    let ModuleOperation32::Pending(attach) =
        engine.load_library_module("helper.dll", None).unwrap()
    else {
        panic!("helper attach")
    };
    assert_eq!(
        return_initializer(&mut engine, attach, 1),
        ModuleOperation32::Loaded { handle: DLL }
    );
    let outer = begin_create(&mut engine, atom, &[WindowsApi32::ExitProcess.id()]);
    call(&mut engine, WindowsApi32::ExitProcess.id(), &[23]);
    let detach = engine.module_initializer().unwrap().unwrap();
    assert_eq!(detach.reason, 0);
    assert_eq!(engine.modules.parked_callback_depth(), 1);
    assert_eq!(engine.exit_code, None);
    loader_material(&mut engine, detach, false);
    assert_eq!(
        return_initializer(&mut engine, detach, 1),
        ModuleOperation32::Released
    );
    assert_eq!(engine.exit_code, Some(23));
    assert!(
        engine.callback.is_none()
            && engine.windows_replies.is_empty()
            && engine.pending_call.is_none()
    );
    assert!(
        engine
            .finish_windows_callback(KEY, outer.generation, outer.token)
            .is_err()
    );
}
