use super::*;
use crate::input::{
    InputKind,
    source::{SourceEvent32, SourceKind32},
};

fn focus(context: &mut User32Context32, sequence: u64, focused: bool) -> Result<(), User32Error32> {
    context.deliver_input(
        SourceEvent32 {
            sequence,
            timestamp_ns: sequence * 1_000_000,
            kind: SourceKind32::Physical(InputKind::Focus { focused }),
            ..Default::default()
        },
        0,
    )
}

fn shown() -> (User32Context32, AddressSpace, User32CallContext32, u32, u32) {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    focus(&mut context, 1, true).unwrap();
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(
        messages.iter().filter(|message| message[1] == 0x1c).count(),
        2
    );
    (context, memory, call, first, second)
}

fn drain(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    mut outcome: User32Outcome32,
) -> (User32Outcome32, Vec<[u32; 4]>) {
    let mut messages = Vec::new();
    while let User32Outcome32::Callback(callback) = outcome {
        messages.push(callback.arguments);
        outcome = context.complete(callback.continuation, 0, memory).unwrap();
    }
    (outcome, messages)
}

#[test]
fn physical_blur_and_restore_are_sent_before_filtered_peek_without_becoming_messages() {
    let (mut context, mut memory, call, first, second) = shown();
    context.input.query_focus = true;
    focus(&mut context, 2, false).unwrap();
    focus(&mut context, 3, true).unwrap();
    assert_eq!(context.wait_queue_status(call.thread_id).0 & 0x40, 0x40);
    memory.write(GuestAddress(0x10200), &[0xa5; 28]).unwrap();
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(
        messages,
        [
            [first, 0x1c, 0, 0],
            [second, 0x1c, 0, 0],
            [first, 0x1c, 1, 0],
            [second, 0x1c, 1, 0],
        ]
    );
    assert_eq!(outcome, User32Outcome32::Return(0));
    let mut output = [0; 28];
    memory.read(GuestAddress(0x10200), &mut output).unwrap();
    assert_eq!(output, [0xa5; 28]);
    assert_eq!(context.wait_queue_status(call.thread_id).0 & 0x40, 0);
}

#[test]
fn activation_wakes_a_parked_get_message_and_resumes_the_original_wait() {
    let (mut context, mut memory, call, first, second) = shown();
    let outcome = context
        .prepare_arguments(
            User32Api32::GetMessageW,
            &[0x10200, 0, 0x400, 0x400],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!("initially empty wait")
    };
    assert!(!context.active_thread_wait_ready());
    focus(&mut context, 2, false).unwrap();
    assert!(context.active_thread_wait_ready());
    let outcome = context.resume_wait(continuation, &mut memory).unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(messages, [[first, 0x1c, 0, 0], [second, 0x1c, 0, 0]]);
    assert!(matches!(outcome, User32Outcome32::Wait { .. }));
    assert!(!context.active_thread_wait_ready());
}

#[test]
fn a_different_gui_thread_cannot_consume_or_execute_another_threads_notifications() {
    let (mut context, mut memory, call, first, second) = shown();
    focus(&mut context, 2, false).unwrap();
    let other = User32CallContext32 {
        thread_id: call.thread_id + 1,
        ..call
    };
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            other,
        )
        .unwrap();
    assert_eq!(outcome, User32Outcome32::Return(0));
    assert_eq!(context.wait_queue_status(call.thread_id).0 & 0x40, 0x40);
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(messages, [[first, 0x1c, 0, 0], [second, 0x1c, 0, 0]]);
    assert_eq!(outcome, User32Outcome32::Return(0));
}

#[test]
fn reentrant_destruction_reclaims_queued_target_and_does_not_wedge_original_peek() {
    let (mut context, mut memory, call, first, second) = shown();
    focus(&mut context, 2, false).unwrap();
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("activation callback")
    };
    assert_eq!(callback.arguments, [first, 0x1c, 0, 0]);
    let nested = context
        .prepare_arguments(User32Api32::DestroyWindow, &[second], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, nested);
    let outcome = context
        .complete(callback.continuation, 0, &mut memory)
        .unwrap();
    assert_eq!(outcome, User32Outcome32::Return(0));
    assert_eq!(context.wait_queue_status(call.thread_id).0 & 0x40, 0);
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(second).unwrap())
            .is_err()
    );
}

#[test]
fn notification_capacity_failure_preserves_the_delivered_physical_state_and_sent_queue() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    create(&mut context, &mut memory, call, atom);
    create(&mut context, &mut memory, call, atom);
    focus(&mut context, 1, true).unwrap();
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    focus(&mut context, 2, false).unwrap();
    focus(&mut context, 3, true).unwrap();
    let before = context.wait_queue_status(call.thread_id);
    assert!(matches!(
        focus(&mut context, 4, false),
        Err(User32Error32::Capacity)
    ));
    assert!(context.input.physical_focus);
    assert_eq!(context.wait_queue_status(call.thread_id), before);
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(outcome, User32Outcome32::Return(0));
    assert_eq!(messages.len(), 6);
    assert_eq!(context.wait_queue_status(call.thread_id).0 & 0x40, 0);
}

#[test]
fn a_window_shown_before_physical_focus_receives_activation_on_later_restore() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert!(messages.iter().all(|message| message[1] != 0x1c));
    focus(&mut context, 1, true).unwrap();
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(messages, [[window, 0x1c, 1, 0]]);
    assert_eq!(outcome, User32Outcome32::Return(0));
}

#[test]
fn destroying_active_window_preserves_application_identity_for_surviving_top_levels() {
    let (mut context, mut memory, call, first, second) = shown();
    let outcome = context
        .prepare_arguments(User32Api32::DestroyWindow, &[first], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    focus(&mut context, 2, false).unwrap();
    let outcome = context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[0x10200, 0, 0x400, 0x400, 0],
            &mut memory,
            call,
        )
        .unwrap();
    let (outcome, messages) = drain(&mut context, &mut memory, outcome);
    assert_eq!(messages, [[second, 0x1c, 0, 0]]);
    assert_eq!(outcome, User32Outcome32::Return(0));
}

#[test]
fn destroying_last_top_level_does_not_suppress_a_later_new_window_activation() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    focus(&mut context, 1, true).unwrap();
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    let outcome = context
        .prepare_arguments(User32Api32::DestroyWindow, &[first], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    let second = create(&mut context, &mut memory, call, atom);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[second, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert!(messages.contains(&[second, 0x1c, 1, 0]));
}
