use super::*;

#[test]
fn queue_publishes_legacy_28_byte_msg_before_removal_and_preserves_adjacent_canary() {
    let (mut context, mut memory, call) = fixture();
    memory.write(GuestAddress(0x10200), &[0xcc; 32]).unwrap();
    context
        .prepare_arguments(
            User32Api32::PostMessageA,
            &[0, 0x400, 0x123, 0x456],
            &mut memory,
            call,
        )
        .unwrap();
    assert!(matches!(
        context.prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x30000, 0, 0, 0, 1],
            &mut memory,
            call
        ),
        Err(User32Error32::Memory(_))
    ));
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::PeekMessageA,
                &[0x10200, 0, 0, 0, 1],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let mut bytes = [0; 32];
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(
        &bytes[..16],
        &[0, 0, 0, 0, 0, 4, 0, 0, 0x23, 1, 0, 0, 0x56, 4, 0, 0]
    );
    assert_eq!(&bytes[28..], &[0xcc; 4]);
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::PeekMessageA,
                &[0x10200, 0, 0, 0, 1],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(0)
    );
}

#[test]
fn blocking_getmessage_waits_retries_and_delivers_quit_despite_message_filter() {
    let (mut context, mut memory, call) = fixture();
    let User32Outcome32::Wait { continuation } = context
        .prepare_arguments(
            User32Api32::GetMessageA,
            &[0x10200, 0, 0x400, 0x500],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Wait { continuation }
    );
    context
        .prepare_arguments(User32Api32::PostQuitMessage, &[42], &mut memory, call)
        .unwrap();
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x12, 42, 0]
    );
    assert_eq!(
        context.resume_wait(continuation, &mut memory),
        Err(User32Error32::InvalidToken)
    );
}

#[test]
fn dispatch_returns_guest_callback_result_and_translate_posts_character() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    let message = crate::windows::user32::Message32 {
        window,
        message: 0x100,
        wparam: 0x41,
        lparam: 1,
        thread_id: 7,
        ..Default::default()
    };
    super::super::memory::write_message(&mut memory, 0x10200, message).unwrap();
    assert_eq!(
        context
            .prepare_arguments(User32Api32::TranslateMessage, &[0x10200], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    context
        .prepare_arguments(
            User32Api32::PeekMessageW,
            &[0x10300, window, 0, 0, 1],
            &mut memory,
            call,
        )
        .unwrap();
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10300).unwrap(),
        [window, 0x102, b'a' as u32, 1]
    );
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(User32Api32::DispatchMessageA, &[0x10300], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(callback.arguments, [window, 0x102, b'a' as u32, 1]);
    assert_eq!(
        context
            .complete(callback.continuation, 0x1234, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0x1234)
    );
}

#[test]
fn post_quit_coalesces_outside_full_normal_queue_and_peek_does_not_consume_it() {
    let (mut context, mut memory, call) = fixture();
    for value in 0..8 {
        assert_eq!(
            context
                .prepare_arguments(
                    User32Api32::PostMessageA,
                    &[0, 0x400, value, 0],
                    &mut memory,
                    call
                )
                .unwrap(),
            User32Outcome32::Return(1)
        );
    }
    context
        .prepare_arguments(User32Api32::PostQuitMessage, &[1], &mut memory, call)
        .unwrap();
    assert_eq!(context.take_last_error(), None);
    context
        .prepare_arguments(User32Api32::PostQuitMessage, &[99], &mut memory, call)
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            context
                .prepare_arguments(
                    User32Api32::PeekMessageA,
                    &[0x10200, 0, 0x500, 0x600, 0],
                    &mut memory,
                    call
                )
                .unwrap(),
            User32Outcome32::Return(1)
        );
        assert_eq!(
            super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
            [0, 0x12, 99, 0]
        );
    }
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::GetMessageA,
                &[0x10200, 0, 0x500, 0x600],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::PeekMessageA,
                &[0x10200, 0, 0x400, 0x400, 1],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x400, 0, 0]
    );
}
