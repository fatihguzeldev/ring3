use super::*;
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::{
        EngineInstance, HostError,
        windows::{
            commdlg::tests::{call, engine},
            user32::{User32Callback32, User32Outcome32},
        },
    },
    windows::{
        commdlg::{DIALOG_PROCEDURE_A, DIALOG_PROCEDURE_W},
        controls::{BUILTIN_PROC_A, BUILTIN_PROC_W},
        user32::{User32Api32 as Api, User32Error32 as E},
    },
};
const OFN: u32 = 0x60000;
const FILE: u32 = OFN + 0x100;
const HOOK: u32 = 0x50100;
fn request(engine: &mut EngineInstance, hook: bool) {
    engine
        .replace_provider_file(
            &"C:\\file.txt".encode_utf16().collect::<Vec<_>>(),
            b"owned file",
        )
        .unwrap();
    let mut bytes = [0; 88];
    for (offset, value) in [
        (0, 88u32),
        (28, FILE),
        (32, 128),
        (12, OFN + 0x400),
        (24, 1),
        (52, 0x80000 | 0x1000 | if hook { 0x20 } else { 0 }),
        (68, if hook { HOOK } else { 0 }),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN), &bytes)
        .unwrap();
    let filters = "Text\0*.txt\0\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 0x400), &filters)
        .unwrap();
    let file = "C:\\file.txt\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(FILE), &file)
        .unwrap();
}
fn pending(engine: &EngineInstance) -> User32Outcome32 {
    let callback = engine.pending_windows_callback().unwrap();
    User32Outcome32::Callback(User32Callback32 {
        procedure: callback.procedure,
        arguments: callback.arguments[..4].try_into().unwrap(),
        continuation: callback.continuation,
        count: callback.count,
    })
}
fn execute(
    engine: &mut EngineInstance,
    callback: User32Callback32,
    events: &mut Vec<(u32, u32)>,
) -> Result<u32, HostError> {
    let args = callback.arguments;
    if callback.procedure.0 == HOOK {
        let code = if args[1] == 0x4e {
            let mut raw = [0; 4];
            engine
                .memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(args[3] + 8), &mut raw)
                .unwrap();
            u32::from_le_bytes(raw)
        } else {
            args[1]
        };
        events.push((args[0], code));
        assert_eq!(args[1], if code == 0x110 { 0x110 } else { 0x4e });
        return Ok(1);
    }
    let id = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.entry == callback.procedure)
        .map(|gate| gate.id)
        .expect("actual mapped system procedure");
    let context = engine.user32_call_context()?;
    let outcome = match id {
        DIALOG_PROCEDURE_A | DIALOG_PROCEDURE_W => {
            engine.execute_common_file_dialog_procedure(&args, id == DIALOG_PROCEDURE_W)?
        }
        BUILTIN_PROC_A | BUILTIN_PROC_W => {
            engine.execute_builtin_control_message(&args, id == BUILTIN_PROC_W, context)?
        }
        _ => {
            let user32 = engine.user32.as_mut().unwrap();
            match Api::from_id(id) {
                Some(Api::DefDlgProcA | Api::DefDlgProcW) => user32
                    .def_dialog_proc(
                        &args,
                        id == Api::DefDlgProcW.id(),
                        engine.memory.as_mut().unwrap(),
                        context,
                    )
                    .map_err(HostError::User32)?,
                Some(Api::DefWindowProcA | Api::DefWindowProcW) => user32
                    .def_window_proc_encoded(
                        &args,
                        id == Api::DefWindowProcW.id(),
                        engine.memory.as_mut().unwrap(),
                        context,
                    )
                    .map_err(HostError::User32)?,
                _ => panic!("unexpected procedure id {id:x}"),
            }
        }
    };
    settle(engine, outcome, events)
}
fn settle(
    engine: &mut EngineInstance,
    mut outcome: User32Outcome32,
    events: &mut Vec<(u32, u32)>,
) -> Result<u32, HostError> {
    for _ in 0..256 {
        match outcome {
            User32Outcome32::Return(value) => return Ok(value),
            User32Outcome32::Callback(callback) => {
                let value = execute(engine, callback, events)?;
                outcome = engine.user32.as_mut().unwrap().complete_with_gdi(
                    callback.continuation,
                    value,
                    engine.memory.as_mut().unwrap(),
                    engine.gdi.as_mut(),
                )?;
            }
            User32Outcome32::Wait { .. } => return Ok(u32::MAX),
        }
    }
    panic!("bounded canonical dialog creation");
}
fn initialized(hook: bool) -> (EngineInstance, u64, WindowHandle32, Vec<(u32, u32)>) {
    initialized_api(0x00210002, 0x1000, hook)
}
fn initialized_api(
    api: u32,
    flags: u32,
    hook: bool,
) -> (EngineInstance, u64, WindowHandle32, Vec<(u32, u32)>) {
    let mut engine = engine();
    request(&mut engine, hook);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(OFN + 52),
            &(flags | 0x80000 | if hook { 0x20 } else { 0 }).to_le_bytes(),
        )
        .unwrap();
    call(&mut engine, api, &[OFN]).unwrap();
    let outcome = pending(&engine);
    let mut events = Vec::new();
    assert_eq!(settle(&mut engine, outcome, &mut events).unwrap(), u32::MAX);
    let plan = &engine.user32.as_ref().unwrap().file_dialogs.plans[0];
    assert!(plan.initialized);
    let token = plan.token;
    let root = plan.root.unwrap();
    (engine, token, root, events)
}
#[test]
fn captured_open_builds_canonical_controls_and_delivers_owned_hook_notifications() {
    let (engine, _, root, events) = initialized(true);
    let context = engine.user32.as_ref().unwrap();
    let files = context.file_dialog_control(root, template::FILES).unwrap();
    assert!(
        context
            .state
            .control_state(files)
            .unwrap()
            .items
            .iter()
            .any(|name| name == &"file.txt".encode_utf16().collect::<Vec<_>>())
    );
    let hook = context.file_dialog_control(root, template::HOOK).unwrap();
    assert_eq!(
        events,
        [
            (hook.get(), 0x110),
            (hook.get(), (-601i32) as u32),
            (hook.get(), (-603i32) as u32)
        ]
    );
    assert_eq!(context.state.record(hook).unwrap().parent, Some(root));
    assert_eq!(
        engine.configured_file_current_directory().unwrap(),
        "C:\\".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn fileok_output_is_visible_before_real_hook_veto_and_second_accept_ends_same_modal_token() {
    let (mut engine, token, root, _) = initialized(true);
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("real FILEOK hook callback")
    };
    let mut info = [0; 20];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(callback.arguments[3]), &mut info)
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(info[8..12].try_into().unwrap()),
        (-606i32) as u32
    );
    assert_eq!(u32::from_le_bytes(info[12..16].try_into().unwrap()), OFN);
    let mut offsets = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(OFN + 56), &mut offsets)
        .unwrap();
    assert_eq!(offsets, [3, 0, 8, 0]);
    let hook = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::HOOK)
        .unwrap();
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .set_extra(hook, 0, 1)
        .unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(callback.continuation, 1, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        None
    );
    assert_eq!(
        engine.user32.as_ref().unwrap().file_dialogs.plans[0].token,
        token
    );
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!()
    };
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(callback.continuation, 0, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        Some(1)
    );
}
#[test]
fn output_protection_failure_keeps_modal_and_selection_then_repair_can_publish_once() {
    let (mut engine, _, root, _) = initialized(false);
    let range = PageRange::new(GuestAddress(OFN), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(matches!(
        engine.execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true),
        Err(HostError::Comdlg(
            crate::windows::commdlg::ComdlgError32::Memory(_)
        ))
    ));
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        None
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        engine
            .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        Some(1)
    );
}
#[test]
fn native_cancel_retains_guest_buffers_and_does_not_create_or_open_files() {
    let (mut engine, _, root, _) = initialized(false);
    let before = engine
        .provider_path_info(&"C:\\file.txt".encode_utf16().collect::<Vec<_>>())
        .unwrap();
    let mut bytes = [0; 256];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut bytes)
        .unwrap();
    assert_eq!(
        engine
            .execute_common_file_dialog_procedure(&[root.get(), 0x111, 2, 0], true)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let mut after = [0; 256];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut after)
        .unwrap();
    assert_eq!(after, bytes);
    assert_eq!(
        engine
            .provider_path_info(&"C:\\file.txt".encode_utf16().collect::<Vec<_>>())
            .unwrap(),
        before
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        Some(0)
    );
}

#[test]
fn overwrite_prompt_is_an_owned_real_response_and_negative_answer_preserves_modal_output() {
    let (mut engine, _, root, _) = initialized_api(0x00210004, 2, false);
    let mut before = [0; 256];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut before)
        .unwrap();
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!("real overwrite confirmation")
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    assert_eq!(
        (request.owner, request.thread, request.style),
        (root.get(), 1, 0x34)
    );
    let (epoch, id) = (request.epoch, request.id);
    engine.complete_user32_dialog(epoch, id, 7, 0).unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        None
    );
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
    let mut after = [0; 256];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut after)
        .unwrap();
    assert_eq!(after, before);
}
#[test]
fn accepted_overwrite_response_survives_output_fault_then_reuses_same_identity_for_fileok_hook() {
    let (mut engine, _, root, _) = initialized_api(0x00210004, 2, true);
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!()
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    engine.complete_user32_dialog(epoch, id, 6, 0).unwrap();
    let range = PageRange::new(GuestAddress(OFN), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(matches!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap()),
        Err(E::Memory(_))
    ));
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    let context = engine.user32.as_ref().unwrap();
    assert_eq!(context.pending.last().unwrap().token, continuation);
    assert!(
        context
            .dialogs
            .iter()
            .any(|dialog| dialog.id == id && dialog.result == Some((6, 0)))
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .resume_wait(continuation, engine.memory.as_mut().unwrap())
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("FILEOK after the same accepted confirmation")
    };
    assert_eq!(callback.procedure.0, HOOK);
    assert_eq!(callback.arguments[1], 0x4e);
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(callback.continuation, 0, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended,
        Some(1)
    );
}
#[test]
fn absent_create_target_is_not_created_by_confirmation_or_path_return() {
    let (mut engine, _, root, _) = initialized_api(0x00210002, 0x2000, false);
    let edit = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::NAME)
        .unwrap();
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .set_text(edit, "new.txt".encode_utf16().collect())
        .unwrap();
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!()
    };
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    let (epoch, id) = (request.epoch, request.id);
    assert_eq!(request.style, 0x34);
    engine.complete_user32_dialog(epoch, id, 6, 0).unwrap();
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .resume_wait(continuation, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine.provider_path_info(&r"C:\new.txt".encode_utf16().collect::<Vec<_>>()),
        Err(HostError::File(crate::files::FileError32::NotFound))
    );
    let mut output = [0; 22];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut output)
        .unwrap();
    assert_eq!(
        output,
        r"C:\new.txt"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .as_slice()
    );
}

#[test]
fn save_nochangedir_finishes_real_modal_destruction_before_atomic_cwd_restore_and_retry() {
    let (mut engine, token, root, mut events) = initialized_api(0x00210004, 8, false);
    engine
        .mount_directory(
            &r"C:\other".encode_utf16().collect::<Vec<_>>(),
            0x10,
            Default::default(),
        )
        .unwrap();
    let edit = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::NAME)
        .unwrap();
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .set_text(edit, r"C:\other".encode_utf16().collect())
        .unwrap();
    assert_eq!(
        engine
            .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        engine.configured_file_current_directory().unwrap(),
        r"C:\other".encode_utf16().collect::<Vec<_>>()
    );
    engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 2, 0], true)
        .unwrap();
    let mut outcome = User32Outcome32::Wait {
        continuation: token,
    };
    let mut finished = false;
    for _ in 0..256 {
        outcome = match outcome {
            User32Outcome32::Wait { continuation } => engine
                .user32
                .as_mut()
                .unwrap()
                .resume_wait(continuation, engine.memory.as_mut().unwrap())
                .unwrap(),
            User32Outcome32::Callback(callback) => {
                let value = execute(&mut engine, callback, &mut events).unwrap();
                engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .complete_with_gdi(
                        callback.continuation,
                        value,
                        engine.memory.as_mut().unwrap(),
                        engine.gdi.as_mut(),
                    )
                    .unwrap()
            }
            User32Outcome32::Return(value) => {
                assert_eq!(value, 0);
                finished = true;
                break;
            }
        };
    }
    assert!(finished);
    assert!(engine.user32.as_ref().unwrap().windows().get(root).is_err());
    let plan = &engine.user32.as_ref().unwrap().file_dialogs.plans[0];
    assert!(plan.completed);
    let (owner, call_token) = (plan.call_owner, plan.call_token);
    let peb = engine.process_peb_address().unwrap();
    let range = PageRange::new(GuestAddress(peb & !4095), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(
        engine
            .finish_commdlg_publication(owner, call_token)
            .is_err()
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(
        engine.configured_file_current_directory().unwrap(),
        r"C:\other".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        engine.user32.as_ref().unwrap().file_dialogs.plans[0].token,
        token
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    engine
        .finish_commdlg_publication(owner, call_token)
        .unwrap();
    assert_eq!(
        engine.configured_file_current_directory().unwrap(),
        r"C:\".encode_utf16().collect::<Vec<_>>()
    );
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .file_dialogs
            .plans
            .is_empty()
    );
}
