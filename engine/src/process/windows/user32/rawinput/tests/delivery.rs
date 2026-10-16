use super::*;
use crate::{
    process::windows::user32::User32Outcome32,
    windows::{
        rawinput::KEYBOARD,
        user32::{Message32, User32Api32, User32Error32},
    },
};
#[test]
fn no_legacy_preserves_physical_key_state_and_raw_data_cleanup_identity() {
    let (mut context, mut memory, call, window) = fixture();
    register(&mut context, &mut memory, 6, 0x30, window);
    key(&mut context, 0x1e);
    assert!(
        context
            .state
            .find_message(7, window, 0x100, 0x100)
            .unwrap()
            .is_none()
    );
    assert_eq!(context.input.asynchronous[65] & 0x80, 0x80);
    let (_, message) = context
        .state
        .find_message(7, window, 0xff, 0xff)
        .unwrap()
        .unwrap();
    assert_eq!(context.state.queue_status(7).0 & 0x400, 0x400);
    context
        .prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x10600, window, 0xff, 0xff, 1],
            &mut memory,
            call,
        )
        .unwrap();
    assert_eq!(
        context.prepare_arguments(User32Api32::GetKeyState, &[65], &mut memory, call),
        Ok(User32Outcome32::Return(0))
    );
    memory
        .write(GuestAddress(0x10400), &40u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        context.raw_input_call(
            Api::GetRawInputData,
            &[message.lparam, 0x10000003, 0x10500, 0x10400, 16],
            &mut memory,
            7
        ),
        Ok(32)
    );
    assert_eq!(dword(&memory, 0x10508), KEYBOARD);
    assert_eq!(dword(&memory, 0x10518), 0x100);
    assert_eq!(
        context.raw_input_call(
            Api::GetRawInputData,
            &[message.lparam, 0x10000003, 0x10500, 0x10400, 16],
            &mut memory,
            8
        ),
        Err(User32Error32::Win32(6))
    );
    assert_eq!(
        context.prepare_arguments(
            User32Api32::DefWindowProcW,
            &[window, 0xff, 0, message.lparam],
            &mut memory,
            call
        ),
        Ok(User32Outcome32::Return(0))
    );
    assert!(context.raw_input.packet(message.lparam, 7).is_err());
}
#[test]
fn raw_message_precedes_legacy_but_only_legacy_read_changes_queued_keyboard_state() {
    let (mut context, mut memory, call, window) = fixture();
    register(&mut context, &mut memory, 6, 0, window);
    key(&mut context, 0x1e);
    assert_eq!(
        context.prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x10600, window, 0, 0, 1],
            &mut memory,
            call
        ),
        Ok(User32Outcome32::Return(1))
    );
    assert_eq!(dword(&memory, 0x10604), 0xff);
    assert_eq!(
        context.prepare_arguments(User32Api32::GetKeyState, &[65], &mut memory, call),
        Ok(User32Outcome32::Return(0))
    );
    assert_eq!(
        context.prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x10600, window, 0, 0, 1],
            &mut memory,
            call
        ),
        Ok(User32Outcome32::Return(1))
    );
    assert_eq!(dword(&memory, 0x10604), 0x100);
    assert_eq!(
        context.prepare_arguments(User32Api32::GetKeyState, &[65], &mut memory, call),
        Ok(User32Outcome32::Return(0xffff8000))
    );
}
#[test]
fn queue_backpressure_never_publishes_half_a_legacy_and_raw_pair() {
    let (mut context, mut memory, _, window) = fixture();
    register(&mut context, &mut memory, 6, 0, window);
    for _ in 0..7 {
        context
            .state
            .post(Message32 {
                thread_id: 7,
                message: 0x400,
                ..Default::default()
            })
            .unwrap();
    }
    let event = crate::input::source::SourceEvent32 {
        kind: crate::input::source::SourceKind32::Physical(crate::input::InputKind::Key {
            code: 0x1e,
            pressed: true,
        }),
        ..Default::default()
    };
    assert_eq!(
        context.deliver_input(event, 0),
        Err(User32Error32::Win32(1816))
    );
    assert_eq!(context.input.asynchronous[65], 0);
    assert!(context.raw_input.queued(7).next().is_none());
    let (index, _) = context
        .state
        .find_message(7, 0, 0x400, 0x400)
        .unwrap()
        .unwrap();
    context.state.remove_message(index);
    context.deliver_input(event, 0).unwrap();
    assert!(
        context
            .state
            .find_message(7, window, 0x100, 0x100)
            .unwrap()
            .is_some()
    );
    let (_, message) = context
        .state
        .find_message(7, window, 0xff, 0xff)
        .unwrap()
        .unwrap();
    assert_eq!(message.lparam, 0x58000000);
}
