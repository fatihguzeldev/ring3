use super::*;
use crate::{
    process::windows::user32::{User32Callback32, User32Outcome32},
    windows::user32::User32Api32 as Api,
};

#[test]
fn extended_template_publishes_each_child_help_id_before_init_dialog() {
    let mut engine = engine();
    let mut bytes: Vec<u8> = [
        1u16,
        0xffff,
        0x3344,
        0x1122,
        0,
        0,
        0,
        0x8000,
        1,
        0,
        0,
        100,
        50,
        0,
        0,
        b'D' as u16,
        0,
    ]
    .into_iter()
    .flat_map(u16::to_le_bytes)
    .collect();
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    bytes.extend(
        [
            0x7788u16,
            0x5566,
            0,
            0,
            0,
            0x5001,
            4,
            8,
            20,
            12,
            100,
            0,
            0xffff,
            0x80,
            b'C' as u16,
            0,
            0,
        ]
        .into_iter()
        .flat_map(u16::to_le_bytes),
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), &bytes)
        .unwrap();
    call(
        &mut engine,
        Api::CreateDialogIndirectParamW.id(),
        &[0x400000, DATA, 0, CODE + 0x100, 0xaabbccdd],
    )
    .unwrap();
    let callback = engine.pending_windows_callback().unwrap();
    let mut outcome = User32Outcome32::Callback(User32Callback32 {
        procedure: callback.procedure,
        arguments: callback.arguments[..4].try_into().unwrap(),
        continuation: callback.continuation,
        count: callback.count,
    });
    let mut checked = false;
    for _ in 0..64 {
        let User32Outcome32::Callback(callback) = outcome else {
            break;
        };
        if callback.arguments[1] == 0x110 {
            let context = engine.user32_call_context().unwrap();
            let state = engine.user32.as_ref().unwrap();
            let root = WindowHandle32::try_from(callback.arguments[0]).unwrap();
            let child = state.state.dialog_item(root, 100).unwrap().unwrap();
            for (window, help) in [(root, 0x11223344), (child, 0x55667788)] {
                assert_eq!(
                    engine
                        .user32
                        .as_mut()
                        .unwrap()
                        .prepare_arguments(
                            Api::GetWindowContextHelpId,
                            &[window.get()],
                            engine.memory.as_mut().unwrap(),
                            context
                        )
                        .unwrap(),
                    User32Outcome32::Return(help)
                );
            }
            checked = true;
        }
        outcome = engine
            .user32
            .as_mut()
            .unwrap()
            .complete_with_gdi(
                callback.continuation,
                u32::from(callback.arguments[1] == 0x81),
                engine.memory.as_mut().unwrap(),
                engine.gdi.as_mut(),
            )
            .unwrap();
    }
    assert!(checked);
    assert_eq!(outcome, User32Outcome32::Return(1));
}
