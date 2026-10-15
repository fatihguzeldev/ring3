use super::*;
use crate::windows::user32::Rect32;

#[test]
fn captured_mouse_coordinates_wheel_and_swapped_buttons_keep_physical_authority() {
    let (mut context, mut memory, call, window) = focused();
    context.input.capture = window;
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::SwapMouseButton,
        &[1],
    );
    context
        .deliver_input(
            SourceEvent32 {
                sequence: 1,
                timestamp_ns: 1_000_000,
                kind: SourceKind32::Cursor { x: -100, y: 800 },
                ..Default::default()
            },
            0,
        )
        .unwrap();
    context
        .deliver_input(
            event(
                2,
                InputKind::Button {
                    button: 0,
                    pressed: true,
                },
            ),
            0,
        )
        .unwrap();
    context
        .deliver_input(
            event(
                3,
                InputKind::Relative {
                    dx: 2,
                    dy: -4,
                    wheel: 120,
                },
            ),
            0,
        )
        .unwrap();
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0x204, 0x204, 1],
    );
    let message = super::super::super::memory::message(&memory, 0x10200).unwrap();
    assert_eq!(
        message.lparam,
        ((-110i32 as u32) & 0xffff) | ((780u32 & 0xffff) << 16)
    );
    assert_eq!(message.wparam & 3, 2);
    assert_eq!(context.input.queued[2], 0x80);
    assert_eq!(context.input.queued[1], 0);
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PeekMessageW,
        &[0x10200, window, 0x20a, 0x20a, 1],
    );
    let wheel = super::super::super::memory::message(&memory, 0x10200).unwrap();
    assert_eq!(wheel.wparam >> 16, 120);
    assert_eq!(wheel.point, Point32 { x: -98, y: 796 });
    assert!(!context.input.physical_capture);
}

#[test]
fn cursor_clip_and_visibility_publish_bounded_host_requests_without_granting_focus() {
    let (mut context, mut memory, call, _) = focused();
    context.input.physical_focus = false;
    super::super::super::memory::write_rect(
        &mut memory,
        0x10200,
        Rect32 {
            left: 10,
            top: 20,
            right: 20,
            bottom: 30,
        },
    )
    .unwrap();
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ClipCursor,
            &[0x10200]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::SetCursorPos,
            &[100, 0]
        ),
        1
    );
    assert_eq!(context.input.point, Point32 { x: 19, y: 20 });
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ShowCursor,
            &[0]
        ),
        u32::MAX
    );
    let request = context.input.cursor_request.unwrap();
    assert_eq!(request.changes, 7);
    assert!(!request.visible);
    assert!(!context.input.physical_focus);
    assert!(!context.input.physical_capture);
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::GetCursorPos,
        &[0x10300],
    );
    assert_eq!(
        super::super::super::memory::words::<2>(&memory, 0x10300).unwrap(),
        [19, 20]
    );
    let window = context.input.focus;
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::ScreenToClient,
            &[window, 0x10300]
        ),
        1
    );
    assert_eq!(
        super::super::super::memory::words::<2>(&memory, 0x10300).unwrap(),
        [9, 0]
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetForegroundWindow,
            &[]
        ),
        0
    );
}

#[test]
fn queue_current_and_new_bits_change_on_observation_without_removing_input() {
    let (mut context, mut memory, call, window) = focused();
    key(&mut context, 1, 0x1e, true);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetInputState,
            &[]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetInputState,
            &[]
        ),
        0
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetQueueStatus,
            &[1]
        ),
        1 << 16
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0, 0, 0]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[65]
        ),
        0
    );
    key(&mut context, 2, 0x1e, false);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetQueueStatus,
            &[1]
        ),
        (1 << 16) | 1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0, 0, 1]
        ),
        1
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[65]
        ),
        0xffff8000
    );
}

#[test]
fn filtered_peek_keeps_allpost_new_bit_and_pm_qs_input_selects_only_hardware() {
    let (mut context, mut memory, call, window) = focused();
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PostMessageW,
        &[window, 0x400, 0, 0],
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0x100, 0x109, 0]
        ),
        0
    );
    assert_eq!(context.state.queue_status(call.thread_id).1 & 0x108, 0x100);
    query(
        &mut context,
        &mut memory,
        call,
        User32Api32::PostMessageW,
        &[window, 0x100, 66, 0],
    );
    key(&mut context, 1, 0x1e, true);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10200, window, 0, 0, 1 | (0x1c07 << 16)]
        ),
        1
    );
    assert_eq!(
        super::super::super::memory::message(&memory, 0x10200)
            .unwrap()
            .wparam,
        65
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[65]
        ),
        0xffff8000
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetKeyState,
            &[66]
        ),
        0
    );
}
