use super::*;
use crate::{
    memory::Permissions,
    process::windows::user32::{User32Callback32, User32Outcome32, callback::Continuation32},
    windows::{
        gdi::GdiError32,
        user32::{User32Api32 as Api, User32Error32 as E},
    },
};

fn template(engine: &mut EngineInstance, font: bool, children: bool) {
    let mut words = vec![
        0,
        0x8000,
        0,
        0,
        u16::from(children),
        0,
        0,
        100,
        50,
        0,
        0,
        0x44,
        0,
    ];
    if font {
        words[0] |= 0x40;
        words.extend([9, 0x41, 0x72, 0x69, 0x61, 0x6c, 0]);
    }
    let mut bytes = words
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    if children {
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        for word in [
            0, 0x5001, 0, 0, 4, 8, 20, 12, 100, 0xffff, 0x80, 0x43, 0, 6, 0x1234, 0x5678,
        ] {
            bytes.extend_from_slice(&u16::to_le_bytes(word));
        }
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), &bytes)
        .unwrap();
}

fn initial(engine: &EngineInstance) -> User32Outcome32 {
    let callback = engine.pending_windows_callback().unwrap();
    User32Outcome32::Callback(User32Callback32 {
        procedure: callback.procedure,
        arguments: callback.arguments[..4].try_into().unwrap(),
        continuation: callback.continuation,
        count: callback.count,
    })
}

fn drive(
    engine: &mut EngineInstance,
    mut outcome: User32Outcome32,
    modal: bool,
    reject: bool,
) -> (u32, Vec<(u32, u32)>) {
    let mut messages = Vec::new();
    for _ in 0..128 {
        let callback = match outcome {
            User32Outcome32::Callback(callback) => callback,
            User32Outcome32::Return(result) => return (result, messages),
            User32Outcome32::Wait { continuation } => {
                let context = engine.user32.as_mut().unwrap();
                assert!(context.resource_dialog_ready(continuation));
                outcome = context
                    .resume_wait(continuation, engine.memory.as_mut().unwrap())
                    .unwrap();
                continue;
            }
        };
        let [window, message, _, parameter] = callback.arguments;
        messages.push((window, message));
        if message == 1 && parameter != 0 && window != 1 {
            let create = crate::process::windows::user32::memory::words::<12>(
                engine.memory.as_ref().unwrap(),
                parameter,
            )
            .unwrap();
            let mut creation = [0; 6];
            engine
                .memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(create[0]), &mut creation)
                .unwrap();
            assert_eq!(creation, [6, 0, 0x34, 0x12, 0x78, 0x56]);
        }
        let result = if message == 0x81 {
            u32::from(!reject)
        } else if message == 0x110 {
            assert_eq!(parameter, 0xaabbccdd);
            assert_eq!(
                engine
                    .user32
                    .as_ref()
                    .unwrap()
                    .state
                    .dialog_item(WindowHandle32::try_from(window).unwrap(), 100)
                    .unwrap()
                    .is_some(),
                !reject
            );
            if modal {
                let context = engine.user32_call_context().unwrap();
                assert_eq!(
                    engine
                        .user32
                        .as_mut()
                        .unwrap()
                        .prepare_arguments(
                            Api::EndDialog,
                            &[window, 0x12345678],
                            engine.memory.as_mut().unwrap(),
                            context
                        )
                        .unwrap(),
                    User32Outcome32::Return(1)
                );
            }
            1
        } else {
            0
        };
        outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .complete_with_gdi(
                callback.continuation,
                result,
                engine.memory.as_mut().unwrap(),
                engine.gdi.as_mut(),
            )
            .unwrap();
    }
    panic!("dialog continuation did not finish");
}

#[test]
fn modeless_and_modal_children_are_created_before_init_and_destroy_in_native_order() {
    for modal in [false, true] {
        let mut engine = engine();
        template(&mut engine, false, true);
        call(
            &mut engine,
            if modal {
                Api::DialogBoxIndirectParamW.id()
            } else {
                Api::CreateDialogIndirectParamW.id()
            },
            &[0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd],
        )
        .unwrap();
        let outcome = initial(&engine);
        let (result, messages) = drive(&mut engine, outcome, modal, false);
        assert_eq!(result, if modal { 0x12345678 } else { 1 });
        let init = messages
            .iter()
            .position(|&event| event == (1, 0x110))
            .unwrap();
        let child_create = messages.iter().position(|&event| event == (2, 1)).unwrap();
        assert!(child_create < init);
        if modal {
            let destroyed = messages
                .iter()
                .copied()
                .filter(|&(_, message)| matches!(message, 2 | 0x82))
                .collect::<Vec<_>>();
            assert_eq!(destroyed, [(1, 2), (2, 2), (2, 0x82), (1, 0x82)]);
            assert_eq!(engine.user32_windows().unwrap().live_windows(), 0);
        } else {
            assert_eq!(engine.user32_windows().unwrap().live_windows(), 2);
        }
        assert!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .resource_dialogs
                .plans
                .is_empty()
        );
        assert!(
            engine
                .user32
                .as_ref()
                .unwrap()
                .thread_scratch_ranges()
                .next()
                .is_none()
        );
    }
}

#[test]
fn rejected_first_nonclient_creation_retires_its_unpublished_template_font() {
    let mut engine = engine();
    template(&mut engine, true, false);
    call(
        &mut engine,
        Api::CreateDialogIndirectParamW.id(),
        &[0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd],
    )
    .unwrap();
    let font = engine.user32.as_ref().unwrap().resource_dialogs.plans[0].font;
    assert_ne!(font, 0);
    let outcome = initial(&engine);
    let (result, messages) = drive(&mut engine, outcome, false, true);
    assert_eq!(result, 0);
    assert!(!messages.iter().any(|&(_, message)| message == 0x110));
    assert_eq!(
        engine.gdi.as_mut().unwrap().retire_dialog_font(font),
        Err(GdiError32::InvalidHandle)
    );
}

#[test]
fn replaced_template_scratch_cannot_change_a_ready_creation_callback() {
    let mut engine = engine();
    template(&mut engine, false, false);
    call(
        &mut engine,
        Api::CreateDialogIndirectParamW.id(),
        &[0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd],
    )
    .unwrap();
    let User32Outcome32::Callback(callback) = initial(&engine) else {
        unreachable!();
    };
    let scratch = engine
        .user32
        .as_ref()
        .unwrap()
        .pending
        .iter()
        .find_map(|pending| match pending.continuation {
            Continuation32::ResourceDialog { scratch, .. } => Some(scratch),
            _ => None,
        })
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap(scratch.range())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(scratch.range(), Permissions::READ_WRITE)
        .unwrap();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let window = engine
        .user32_windows()
        .unwrap()
        .get(WindowHandle32::try_from(1).unwrap())
        .unwrap();
    let pending = engine.user32.as_ref().unwrap().pending.len();
    assert_eq!(
        engine.user32.as_mut().unwrap().complete_with_gdi(
            callback.continuation,
            1,
            engine.memory.as_mut().unwrap(),
            engine.gdi.as_mut()
        ),
        Err(HostError::User32(E::InvalidRequest))
    );
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(
        engine
            .user32_windows()
            .unwrap()
            .get(WindowHandle32::try_from(1).unwrap())
            .unwrap(),
        window
    );
    assert_eq!(engine.user32.as_ref().unwrap().pending.len(), pending);
}

#[test]
fn nested_modeless_creation_returns_to_only_its_exact_outer_init_callback() {
    let mut engine = engine();
    template(&mut engine, false, false);
    call(
        &mut engine,
        Api::CreateDialogIndirectParamW.id(),
        &[0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd],
    )
    .unwrap();
    let mut outcome = initial(&engine);
    let outer = loop {
        let User32Outcome32::Callback(callback) = outcome else {
            panic!("outer init was skipped");
        };
        if callback.arguments[1] == 0x110 {
            break callback;
        }
        let value = u32::from(callback.arguments[1] == 0x81);
        outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .complete_with_gdi(
                callback.continuation,
                value,
                engine.memory.as_mut().unwrap(),
                engine.gdi.as_mut(),
            )
            .unwrap();
    };
    let original = engine.user32.as_ref().unwrap().resource_dialogs.plans[0].token;
    template(&mut engine, false, false);
    let context = engine.user32_call_context().unwrap();
    let input = engine
        .prepare_resource_dialog_creation(
            Api::CreateDialogIndirectParamW,
            &[0x400000, DATA, 0, CODE + 0x100, 0x11223344],
            context,
        )
        .unwrap();
    let mut outcome = engine
        .user32
        .as_mut()
        .unwrap()
        .begin_resource_dialog(input, engine.memory.as_mut().unwrap())
        .unwrap();
    for _ in 0..32 {
        match outcome {
            User32Outcome32::Return(value) => {
                assert_eq!(value, 2);
                break;
            }
            User32Outcome32::Callback(callback) => {
                if callback.arguments[1] == 0x110 {
                    assert_eq!(callback.arguments[3], 0x11223344);
                }
                let value = u32::from(callback.arguments[1] == 0x81);
                outcome = engine
                    .user32
                    .as_mut()
                    .unwrap()
                    .complete_with_gdi(
                        callback.continuation,
                        value,
                        engine.memory.as_mut().unwrap(),
                        engine.gdi.as_mut(),
                    )
                    .unwrap();
            }
            User32Outcome32::Wait { .. } => panic!("modeless dialog waited"),
        }
    }
    assert_eq!(outcome, User32Outcome32::Return(2));
    let context = engine.user32.as_ref().unwrap();
    assert_eq!(context.pending.last().unwrap().token, outer.continuation);
    assert_eq!(context.resource_dialogs.plans.len(), 1);
    assert_eq!(context.resource_dialogs.plans[0].token, original);
    assert_eq!(
        engine
            .user32
            .as_mut()
            .unwrap()
            .complete_with_gdi(
                outer.continuation,
                0,
                engine.memory.as_mut().unwrap(),
                engine.gdi.as_mut()
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(engine.user32_windows().unwrap().live_windows(), 2);
    assert!(engine.user32.as_ref().unwrap().pending.is_empty());
    assert!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .thread_scratch_ranges()
            .next()
            .is_none()
    );
}
