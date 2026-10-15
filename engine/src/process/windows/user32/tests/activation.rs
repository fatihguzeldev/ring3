use super::*;

fn drain_show(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    mut outcome: User32Outcome32,
) -> (u32, Vec<[u32; 4]>) {
    let mut messages = Vec::new();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                messages.push(callback.arguments);
                outcome = context.complete(callback.continuation, 0, memory).unwrap();
            }
            User32Outcome32::Return(value) => return (value, messages),
            _ => panic!("show cannot wait"),
        }
    }
}

#[test]
fn showing_hidden_window_activates_before_focus_and_preserves_previous_visibility_result() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    let handle = WindowHandle32::try_from(window).unwrap();
    let mut outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    for message in [0x18, 0x86, 6, 7] {
        let User32Outcome32::Callback(callback) = outcome else {
            panic!()
        };
        assert_eq!(
            callback.arguments,
            [window, message, u32::from(message != 7), 0]
        );
        assert_eq!(
            context.windows().active(),
            (message != 0x18).then_some(handle)
        );
        assert_eq!(context.windows().focus(), (message == 7).then_some(handle));
        outcome = context
            .complete(callback.continuation, 0, &mut memory)
            .unwrap();
    }
    assert_eq!(outcome, User32Outcome32::Return(0));
    assert!(context.windows().get(handle).unwrap().description.visible);
    assert_eq!(
        context
            .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(1)
    );
}

#[test]
fn noactivate_show_preserves_focus_and_later_show_activates_an_already_visible_window() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    for command in [4, 8] {
        let window = create(&mut context, &mut memory, call, atom);
        let outcome = context
            .prepare_arguments(
                User32Api32::ShowWindow,
                &[window, command],
                &mut memory,
                call,
            )
            .unwrap();
        let (_, messages) = drain_show(&mut context, &mut memory, outcome);
        assert_eq!(messages, [[window, 0x18, 1, 0]]);
        assert_eq!(context.windows().active(), None);
        assert_eq!(context.windows().focus(), None);
    }
    let window = create(&mut context, &mut memory, call, atom);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 8], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    let (result, messages) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(result, 1);
    assert_eq!(
        messages,
        [[window, 0x86, 1, 0], [window, 6, 1, 0], [window, 7, 0, 0]]
    );
}

#[test]
fn changing_active_window_delivers_deactivation_before_new_activation_then_focus() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, outcome);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[second, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(
        messages,
        [
            [second, 0x18, 1, 0],
            [first, 0x86, 0, second],
            [first, 6, 0, second],
            [second, 0x86, 1, first],
            [second, 6, 1, first],
            [first, 8, second, 0],
            [second, 7, first, 0],
        ]
    );
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[second, 0], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(
        messages,
        [
            [second, 0x18, 0, 0],
            [second, 0x86, 0, 0],
            [second, 6, 0, 0],
            [second, 8, 0, 0]
        ]
    );
    assert_eq!(context.windows().active(), None);
    assert_eq!(context.windows().focus(), None);
}

#[test]
fn default_activate_can_focus_inside_wndproc_and_reentrant_activation_keeps_its_owner() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    let mut outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    for _ in 0..2 {
        let User32Outcome32::Callback(callback) = outcome else {
            panic!()
        };
        outcome = context
            .complete(callback.continuation, 0, &mut memory)
            .unwrap();
    }
    let User32Outcome32::Callback(active) = outcome else {
        panic!()
    };
    assert_eq!(active.arguments, [first, 6, 1, 0]);
    let nested = context
        .prepare_arguments(
            User32Api32::DefWindowProcA,
            &active.arguments,
            &mut memory,
            call,
        )
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, nested);
    assert_eq!(messages, [[first, 7, 0, 0]]);
    let nested = context
        .prepare_arguments(User32Api32::ShowWindow, &[second, 5], &mut memory, call)
        .unwrap();
    drain_show(&mut context, &mut memory, nested);
    assert_eq!(
        context
            .complete(active.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(context.windows().active().unwrap().get(), second);
    assert_eq!(context.windows().focus().unwrap().get(), second);
}
