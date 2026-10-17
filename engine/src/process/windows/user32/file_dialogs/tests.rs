use super::callbacks::HookStage32;
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

fn multiple_dialog() -> (EngineInstance, u64, WindowHandle32, Vec<(u32, u32)>) {
    let mut engine = engine();
    request(&mut engine, true);
    engine
        .replace_provider_file(
            &r"C:\second name.txt".encode_utf16().collect::<Vec<_>>(),
            b"second input",
        )
        .unwrap();
    engine
        .replace_provider_file(
            &r"C:\third.txt".encode_utf16().collect::<Vec<_>>(),
            b"third input",
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 52), &0x81220u32.to_le_bytes())
        .unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    let outcome = pending(&engine);
    let mut events = Vec::new();
    assert_eq!(settle(&mut engine, outcome, &mut events).unwrap(), u32::MAX);
    let context = engine.user32.as_ref().unwrap();
    let plan = &context.file_dialogs.plans[0];
    let (token, root) = (plan.token, plan.root.unwrap());
    (engine, token, root, events)
}

#[test]
fn explorer_multi_selection_uses_physical_control_masks_and_atomic_double_nul_file_output() {
    let (mut engine, token, root, mut events) = multiple_dialog();
    let files = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::FILES)
        .unwrap();
    let font = engine
        .user32
        .as_ref()
        .unwrap()
        .state
        .control_state(files)
        .unwrap()
        .font;
    let height = engine
        .gdi
        .as_ref()
        .unwrap()
        .control_input_metrics(font, None)
        .unwrap()
        .0;
    let state = engine
        .user32
        .as_ref()
        .unwrap()
        .state
        .control_state(files)
        .unwrap();
    let rows = ["file.txt", "second name.txt"].map(|name| {
        let expected = name.encode_utf16().collect::<Vec<_>>();
        state
            .items
            .iter()
            .position(|item| item == &expected)
            .expect("literal mounted fixture file in actual ListBox")
    });
    assert!(rows[0] < rows[1]);
    for (row, control) in [(rows[0], false), (rows[1], true)] {
        engine
            .set_user32_keyboard_state(false, control, false)
            .unwrap();
        let position = 3u32 | (((row as i32 * height + 2) as u32) << 16);
        for message in [0x201, 0x202] {
            let context = engine.user32_call_context().unwrap();
            let outcome = engine
                .execute_builtin_control_message(
                    &[files.get(), message, 0, position],
                    true,
                    context,
                )
                .unwrap();
            settle(&mut engine, outcome, &mut events).unwrap();
        }
    }
    let context = engine.user32_call_context().unwrap();
    let count = engine
        .execute_builtin_control_message(&[files.get(), 0x190, 0, 0], true, context)
        .unwrap();
    assert_eq!(count, User32Outcome32::Return(2));
    let edit = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::NAME)
        .unwrap();
    assert_eq!(
        engine.user32.as_ref().unwrap().state.text(edit).unwrap(),
        "\"file.txt\" \"second name.txt\""
            .encode_utf16()
            .collect::<Vec<_>>(),
        "actual guest-hook events {events:?}"
    );
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let output = PageRange::new(GuestAddress(OFN), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(output, Permissions::READ)
        .unwrap();
    assert!(matches!(
        engine.execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true),
        Err(HostError::Comdlg(
            crate::windows::commdlg::ComdlgError32::Memory(_)
        ))
    ));
    assert_eq!(
        engine.user32.as_ref().unwrap().file_dialogs.plans[0].token,
        token
    );
    assert!(
        engine.user32.as_ref().unwrap().file_dialogs.plans[0]
            .published_error
            .is_none()
    );
    assert!(engine.memory.as_ref().unwrap().transaction_version() > version);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(
        engine
            .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
            .is_err()
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(output, Permissions::READ_WRITE)
        .unwrap();
    let outcome = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("real FILEOK hook");
    };
    let expected = "C:\\\0file.txt\0second name.txt\0\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let mut bytes = vec![0; expected.len()];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(FILE), &mut bytes)
        .unwrap();
    assert_eq!(bytes, expected);
    let mut offsets = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(OFN + 56), &mut offsets)
        .unwrap();
    assert_eq!(offsets, [4, 0, 0, 0]);
    assert_eq!(callback.arguments[1], 0x4e);
    let plan = &engine.user32.as_ref().unwrap().file_dialogs.plans[0];
    assert_eq!(plan.selection.as_ref().unwrap().additional.len(), 1);
}

#[test]
fn multiselect_queries_follow_same_canonical_items_and_fault_before_guest_index_publication() {
    let (mut engine, _, root, _) = multiple_dialog();
    let files = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::FILES)
        .unwrap();
    for index in [0, 2] {
        let context = engine.user32_call_context().unwrap();
        assert_eq!(
            engine
                .execute_builtin_control_message(&[files.get(), 0x185, 1, index], true, context)
                .unwrap(),
            User32Outcome32::Return(0)
        );
    }
    let context = engine.user32_call_context().unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(
        engine
            .execute_builtin_control_message(&[files.get(), 0x191, 3, 0x6ffffffe], true, context)
            .is_err()
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    let context = engine.user32_call_context().unwrap();
    assert_eq!(
        engine
            .execute_builtin_control_message(&[files.get(), 0x191, 3, OFN + 0x800], true, context)
            .unwrap(),
        User32Outcome32::Return(2)
    );
    let mut bytes = [0; 8];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(OFN + 0x800), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0, 0, 0, 0, 2, 0, 0, 0]);
}

fn common_template_bytes(template: &crate::windows::user32::dialogs::DialogTemplate32) -> Vec<u8> {
    use crate::windows::user32::dialogs::DialogName32;
    fn word(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn dword(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn text(bytes: &mut Vec<u8>, value: &[u16]) {
        for unit in value.iter().copied().chain(std::iter::once(0)) {
            word(bytes, unit);
        }
    }
    fn name(bytes: &mut Vec<u8>, value: &DialogName32) {
        match value {
            DialogName32::Empty => word(bytes, 0),
            DialogName32::Ordinal(id) => {
                word(bytes, 0xffff);
                word(bytes, *id);
            }
            DialogName32::Text(value) => text(bytes, value),
        }
    }
    let mut bytes = Vec::new();
    dword(&mut bytes, template.style);
    dword(&mut bytes, template.ex_style);
    word(&mut bytes, template.items.len() as u16);
    for unit in [
        template.units.x,
        template.units.y,
        template.units.width,
        template.units.height,
    ] {
        word(&mut bytes, unit as u16);
    }
    name(&mut bytes, &template.menu);
    name(&mut bytes, &template.class);
    text(&mut bytes, &template.title);
    if let Some(font) = &template.font {
        word(&mut bytes, font.points);
        text(&mut bytes, &font.face);
    }
    for item in &template.items {
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        dword(&mut bytes, item.style);
        dword(&mut bytes, item.ex_style);
        for unit in [
            item.units.x,
            item.units.y,
            item.units.width,
            item.units.height,
        ] {
            word(&mut bytes, unit as u16);
        }
        word(&mut bytes, item.id as u16);
        name(&mut bytes, &item.class);
        name(&mut bytes, &item.title);
        word(&mut bytes, 0);
    }
    bytes
}

#[test]
fn explorer_custom_template_has_its_own_real_parent_and_font_after_input_block_changes() {
    use crate::windows::user32::dialogs::{DialogFont32, DialogName32};
    let mut engine = engine();
    request(&mut engine, true);
    let mut custom = template::template(
        "Custom options".encode_utf16().collect(),
        false,
        false,
        false,
        false,
        false,
    );
    custom.items.truncate(1);
    custom.items[0].id = 900;
    custom.items[0].title = DialogName32::Text("Frozen option".encode_utf16().collect());
    custom.style |= 0x40;
    custom.font = Some(DialogFont32 {
        points: 9,
        weight: 400,
        italic: false,
        charset: 1,
        face: "Arial".encode_utf16().collect(),
    });
    let bytes = common_template_bytes(&custom);
    let block = engine.allocate_windows_runtime_initialized(&bytes).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 8), &block.to_le_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 52), &0x810a0u32.to_le_bytes())
        .unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(block), &[0xff; 32])
        .unwrap();
    let mut events = Vec::new();
    let outcome = pending(&engine);
    settle(&mut engine, outcome, &mut events).unwrap();
    let context = engine.user32.as_ref().unwrap();
    let root = context.file_dialogs.plans[0].root.unwrap();
    let child = context.file_dialog_control(root, template::HOOK).unwrap();
    let option = context.state.dialog_item(child, 900).unwrap().unwrap();
    assert_eq!(context.state.record(option).unwrap().parent, Some(child));
    assert!(context.state.dialog_item(root, 900).unwrap().is_none());
    assert_eq!(
        context.state.text(option).unwrap(),
        "Frozen option".encode_utf16().collect::<Vec<_>>()
    );
    let font = context.state.dialog(child).unwrap().font;
    assert_ne!(font, 0);
    assert_eq!(context.state.control_state(option).unwrap().font, font);
    assert_eq!(
        events,
        [
            (child.get(), 0x110),
            (child.get(), (-601i32) as u32),
            (child.get(), (-603i32) as u32)
        ]
    );
    assert!(context.file_dialog_control(root, template::NAME).is_ok());
}

#[test]
fn old_style_hook_can_consume_raw_root_messages_and_zero_result_runs_the_same_default_message() {
    let mut engine = engine();
    request(&mut engine, true);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 52), &0x1020u32.to_le_bytes())
        .unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    let mut events = Vec::new();
    let outcome = pending(&engine);
    settle(&mut engine, outcome, &mut events).unwrap();
    let context = engine.user32.as_ref().unwrap();
    let plan = &context.file_dialogs.plans[0];
    let root = plan.root.unwrap();
    let token = plan.token;
    assert_eq!(events, [(root.get(), 0x110)]);
    assert!(
        context
            .state
            .dialog_item(root, template::HOOK)
            .unwrap()
            .is_none()
    );
    let arguments = [root.get(), 0x111, 2, 0];
    let User32Outcome32::Callback(callback) = engine
        .execute_common_file_dialog_procedure(&arguments, true)
        .unwrap()
    else {
        panic!("real legacy hook");
    };
    assert_eq!(callback.procedure.0, HOOK);
    assert_eq!(callback.arguments, arguments);
    let User32Outcome32::Callback(nested) = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x8001, 0, 0], true)
        .unwrap()
    else {
        panic!("reentrant old-style hook is separately owned");
    };
    assert_ne!(nested.continuation, callback.continuation);
    assert_eq!(nested.arguments, [root.get(), 0x8001, 0, 0]);
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(nested.continuation, 1, engine.memory.as_mut().unwrap())
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .complete(callback.continuation, 1, engine.memory.as_mut().unwrap())
        .unwrap();
    assert_eq!(outcome, User32Outcome32::Return(1));
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended
            .is_none()
    );
    let User32Outcome32::Callback(callback) = engine
        .execute_common_file_dialog_procedure(&arguments, true)
        .unwrap()
    else {
        panic!("same legacy message");
    };
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .complete(callback.continuation, 0, engine.memory.as_mut().unwrap())
        .unwrap();
    let User32Outcome32::Callback(resume) = outcome else {
        panic!("default procedure runs only after accepted zero result");
    };
    assert_ne!(resume.procedure.0, HOOK);
    assert_eq!(resume.arguments, arguments);
    assert_eq!(
        settle(&mut engine, User32Outcome32::Callback(resume), &mut events).unwrap(),
        1
    );
    let context = engine.user32.as_ref().unwrap();
    assert_eq!(context.file_dialogs.plans[0].token, token);
    assert_eq!(context.state.dialog(root).unwrap().ended, Some(0));
}

#[test]
fn legacy_fileok_registered_identity_uses_hook_return_without_explorer_dwl_result() {
    let mut engine = engine();
    request(&mut engine, true);
    let template = template::template(
        "Legacy template".encode_utf16().collect(),
        false,
        true,
        false,
        false,
        false,
    );
    let block = engine
        .allocate_windows_runtime_initialized(&common_template_bytes(&template))
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 8), &block.to_le_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN + 52), &0x10a0u32.to_le_bytes())
        .unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    let mut events = Vec::new();
    let outcome = pending(&engine);
    settle(&mut engine, outcome, &mut events).unwrap();
    let context = engine.user32.as_ref().unwrap();
    let plan = &context.file_dialogs.plans[0];
    let (root, token, message) = (
        plan.root.unwrap(),
        plan.token,
        plan.legacy_messages.unwrap()[0],
    );
    assert!(message >= 0xc000);
    assert_eq!(
        context.state.text(root).unwrap(),
        "Legacy template".encode_utf16().collect::<Vec<_>>()
    );
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .file_dialog_hook(token, HookStage32::FileOk, engine.memory.as_mut().unwrap())
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("legacy FILEOKSTRING");
    };
    assert_eq!(callback.arguments, [root.get(), message, 0, OFN]);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .extra(root, 0)
            .unwrap(),
        0
    );
    engine
        .user32
        .as_mut()
        .unwrap()
        .complete(callback.continuation, 1, engine.memory.as_mut().unwrap())
        .unwrap();
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .state
            .dialog(root)
            .unwrap()
            .ended
            .is_none()
    );
    let outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .file_dialog_hook(token, HookStage32::FileOk, engine.memory.as_mut().unwrap())
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("retry FILEOKSTRING");
    };
    engine
        .user32
        .as_mut()
        .unwrap()
        .complete(callback.continuation, 0, engine.memory.as_mut().unwrap())
        .unwrap();
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
fn captured_registered_messages_use_same_ansi_unicode_name_and_do_not_share_class_atom_issuer() {
    let mut engine = engine();
    let name = "commdlg_FileNameOK\0";
    let wide = name
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN), &wide)
        .unwrap();
    call(&mut engine, 0x000200cf, &[OFN]).unwrap();
    let wide = crate::abi::x86::decode_state(&engine.arena()[..crate::abi::x86::STATE_SIZE])
        .unwrap()
        .registers[0];
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(OFN), name.as_bytes())
        .unwrap();
    call(&mut engine, 0x000200ce, &[OFN]).unwrap();
    let ansi = crate::abi::x86::decode_state(&engine.arena()[..crate::abi::x86::STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_eq!(wide, ansi);
    assert!((0xc000..=0xffff).contains(&wide));
    let state = &mut engine.user32.as_mut().unwrap().state;
    assert_ne!(
        state
            .register_message("another message".encode_utf16().collect())
            .unwrap(),
        wide
    );
    assert_eq!(
        state
            .register_message("commdlg_FileNameOK".encode_utf16().collect())
            .unwrap(),
        wide
    );
}

#[test]
fn shown_help_button_issues_owned_explorer_notification_and_ignores_hook_return() {
    let (mut engine, token, root, _) = initialized_api(0x00210002, 0x1010, true);
    let context = engine.user32.as_ref().unwrap();
    let button = context.file_dialog_control(root, template::HELP).unwrap();
    assert_eq!(context.state.record(button).unwrap().parent, Some(root));
    let User32Outcome32::Callback(callback) = engine
        .execute_common_file_dialog_procedure(
            &[root.get(), 0x111, template::HELP, button.get()],
            true,
        )
        .unwrap()
    else {
        panic!("the help button must invoke the actual guest hook")
    };
    assert_eq!(callback.procedure.0, HOOK);
    let info =
        super::super::memory::words::<5>(engine.memory().unwrap(), callback.arguments[3]).unwrap();
    assert_eq!(info, [root.get(), 0, (-605i32) as u32, OFN, 0]);
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(
                callback.continuation,
                u32::MAX,
                engine.memory.as_mut().unwrap()
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let context = engine.user32.as_ref().unwrap();
    assert_eq!(context.file_dialogs.plans[0].token, token);
    assert_eq!(context.state.dialog(root).unwrap().ended, None);
    assert!(context.file_dialogs.plans[0].selection.is_none());
}

#[test]
fn owner_help_uses_the_canonical_sendmessage_gate_with_captured_owner_and_ofn() {
    let mut engine = engine();
    request(&mut engine, false);
    engine.map(0x10000, 2, 3).unwrap();
    let call_context = engine.user32_call_context().unwrap();
    let owner_procedure = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == Api::DefWindowProcA.id())
        .unwrap()
        .entry
        .0;
    let context = engine.user32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    memory
        .write(GuestAddress(0x10100), b"CommonOwner\0")
        .unwrap();
    let mut class = [0u8; 40];
    for (offset, value) in [
        (4, owner_procedure),
        (16, call_context.instance),
        (36, 0x10100),
    ] {
        class[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    memory.write(GuestAddress(0x10000), &class).unwrap();
    let User32Outcome32::Return(atom) = context
        .prepare_arguments(Api::RegisterClassA, &[0x10000], memory, call_context)
        .unwrap()
    else {
        panic!("class registration")
    };
    let owner = super::super::tests::create(context, memory, call_context, atom);
    memory
        .write(GuestAddress(OFN + 4), &owner.to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(OFN + 52), &0x81010u32.to_le_bytes())
        .unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    let outcome = pending(&engine);
    settle(&mut engine, outcome, &mut Vec::new()).unwrap();
    let root = engine.user32.as_ref().unwrap().file_dialogs.plans[0]
        .root
        .unwrap();
    let User32Outcome32::Callback(callback) = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, template::HELP, 0], true)
        .unwrap()
    else {
        panic!("owner notification")
    };
    let gate = engine
        .loaded_windows_modules()
        .flat_map(|module| module.gates())
        .find(|gate| gate.id == Api::SendMessageW.id())
        .map(|gate| gate.entry)
        .unwrap();
    let message = engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .register_message("commdlg_help".encode_utf16().collect())
        .unwrap();
    assert_eq!(callback.procedure, gate);
    assert_eq!(callback.arguments, [owner, message, root.get(), OFN]);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .windows()
            .get(WindowHandle32::try_from(owner).unwrap())
            .unwrap()
            .description
            .thread_id,
        call_context.thread_id
    );
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
        None
    );
}

#[test]
fn sharing_hook_keeps_sealed_fallthrough_across_output_repair_and_then_delivers_fileok() {
    let (mut engine, _, root, _) = initialized_api(0x00210004, 0, true);
    let lease = engine
        .open_provider_file(&r"C:\file.txt".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap();
    let User32Outcome32::Callback(share) = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap()
    else {
        panic!("sharing violation must reach the guest hook")
    };
    let info =
        super::super::memory::words::<5>(engine.memory().unwrap(), share.arguments[3]).unwrap();
    assert_eq!(&info[..4], &[root.get(), 0, (-604i32) as u32, OFN]);
    let name = super::super::memory::string(
        engine.memory().unwrap(),
        info[4],
        true,
        32767,
        crate::windows::text::CodePage32::Windows1252,
    )
    .unwrap();
    assert_eq!(name, r"C:\file.txt".encode_utf16().collect::<Vec<_>>());
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
        .set_extra(hook, 0, 2)
        .unwrap();
    let range = PageRange::new(GuestAddress(OFN), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.user32.as_mut().unwrap().complete(
            share.continuation,
            1,
            engine.memory.as_mut().unwrap()
        ),
        Err(E::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .pending
            .last()
            .unwrap()
            .token,
        share.continuation
    );
    // the accepted decision is owned by the continuation, not by mutable DWLP_MSGRESULT.
    engine
        .user32
        .as_mut()
        .unwrap()
        .state
        .set_extra(hook, 0, 1)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    let User32Outcome32::Callback(fileok) = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap()
    else {
        panic!("sealed fallthrough must advance directly to FILEOK")
    };
    assert_eq!(fileok.continuation, share.continuation);
    assert_eq!(
        super::super::memory::words::<5>(engine.memory().unwrap(), fileok.arguments[3]).unwrap()[2],
        (-606i32) as u32
    );
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete(fileok.continuation, 0, engine.memory.as_mut().unwrap())
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
    engine.close_provider_file(lease).unwrap();
}

#[test]
fn share_nowarn_stays_modal_and_zero_result_opens_real_warning_on_the_same_parent_token() {
    let (mut engine, _, root, _) = initialized_api(0x00210004, 0, true);
    let lease = engine
        .open_provider_file(&r"C:\file.txt".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap();
    let hook = engine
        .user32
        .as_ref()
        .unwrap()
        .file_dialog_control(root, template::HOOK)
        .unwrap();
    for action in [1, 0] {
        let User32Outcome32::Callback(callback) = engine
            .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
            .unwrap()
        else {
            panic!("sharing callback")
        };
        engine
            .user32
            .as_mut()
            .unwrap()
            .state
            .set_extra(hook, 0, action)
            .unwrap();
        let outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .complete(
                callback.continuation,
                if action == 0 { 0 } else { 1 },
                engine.memory.as_mut().unwrap(),
            )
            .unwrap();
        if action == 1 {
            assert_eq!(outcome, User32Outcome32::Return(1));
            assert!(engine.user32_dialog_request(0).unwrap().is_none());
        } else {
            let User32Outcome32::Wait { continuation } = outcome else {
                panic!("owned sharing warning")
            };
            let context = engine.user32.as_ref().unwrap();
            assert_eq!(
                context.pending[context.pending.len() - 2].token,
                callback.continuation
            );
            let request = engine.user32_dialog_request(0).unwrap().unwrap();
            assert_eq!((request.owner, request.style), (root.get(), 0x30));
            let (epoch, id) = (request.epoch, request.id);
            engine.complete_user32_dialog(epoch, id, 1, 0).unwrap();
            assert_eq!(
                engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .resume_wait(continuation, engine.memory.as_mut().unwrap())
                    .unwrap(),
                User32Outcome32::Return(1)
            );
        }
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
        assert!(
            engine.user32.as_ref().unwrap().file_dialogs.plans[0]
                .selection
                .is_none()
        );
    }
    engine.close_provider_file(lease).unwrap();
}

#[test]
fn shareaware_uses_canonical_file_admission_without_publishing_an_open_or_warning() {
    let (mut engine, _, root, _) = initialized_api(0x00210004, 0x4000, false);
    let lease = engine
        .open_provider_file(&r"C:\file.txt".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap();
    let version = engine.provider_file_version(lease).unwrap();
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
    assert!(engine.user32_dialog_request(0).unwrap().is_none());
    assert_eq!(engine.provider_file_version(lease).unwrap(), version);
    engine.close_provider_file(lease).unwrap();
}

#[test]
fn relative_filename_preserves_explicit_initial_directory_and_readonly_return_is_rejected() {
    let mut engine = engine();
    request(&mut engine, false);
    engine
        .provider_create_directories(&r"C:\chosen".encode_utf16().collect::<Vec<_>>())
        .unwrap();
    engine
        .replace_provider_file(
            &r"C:\chosen\file.txt".encode_utf16().collect::<Vec<_>>(),
            b"kept",
        )
        .unwrap();
    let ram = engine.memory.as_mut().unwrap();
    ram.write(
        GuestAddress(FILE),
        &"file.txt\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    ram.write(
        GuestAddress(OFN + 0x700),
        &"C:\\chosen\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    ram.write(GuestAddress(OFN + 44), &(OFN + 0x700).to_le_bytes())
        .unwrap();
    ram.write(GuestAddress(OFN + 52), &0x89000u32.to_le_bytes())
        .unwrap();
    // use the actual file provider to set the on-disk read-only attribute before opening the dialog.
    let attributes = crate::windows::files::FileApi32::SetFileAttributesW.id();
    ram.write(
        GuestAddress(OFN + 0x800),
        &"C:\\chosen\\file.txt\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    call(&mut engine, attributes, &[OFN + 0x800, 1]).unwrap();
    call(&mut engine, 0x00210002, &[OFN]).unwrap();
    let outcome = pending(&engine);
    settle(&mut engine, outcome, &mut Vec::new()).unwrap();
    let plan = &engine.user32.as_ref().unwrap().file_dialogs.plans[0];
    assert_eq!(
        plan.directory,
        r"C:\chosen".encode_utf16().collect::<Vec<_>>()
    );
    let root = plan.root.unwrap();
    let User32Outcome32::Wait { .. } = engine
        .execute_common_file_dialog_procedure(&[root.get(), 0x111, 1, 0], true)
        .unwrap()
    else {
        panic!("read-only selection must produce the real access warning")
    };
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
    let request = engine.user32_dialog_request(0).unwrap().unwrap();
    assert!(String::from_utf16_lossy(request.text).contains("error 5"));
    assert_eq!(
        engine
            .provider_path_info(&r"C:\chosen\file.txt".encode_utf16().collect::<Vec<_>>())
            .unwrap()
            .attributes
            & 1,
        1
    );
}
