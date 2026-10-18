use super::*;

#[test]
fn first_top_level_activation_notifies_application_before_window_activation() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    context.input.physical_focus = true;
    context.input.query_focus = true;
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    let app = messages
        .iter()
        .position(|message| *message == [window, 0x1c, 1, 0])
        .expect("first application activation must reach its window procedure");
    let window_activation = messages.iter().position(|message| message[1] == 6).unwrap();
    assert!(app < window_activation);
    assert_eq!(
        messages.iter().filter(|message| message[1] == 0x1c).count(),
        1
    );
}

#[test]
fn physically_inactive_show_does_not_claim_application_activation() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    assert!(!context.input.physical_focus);
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert!(messages.iter().all(|message| message[1] != 0x1c));
}

#[test]
fn same_thread_top_level_switch_does_not_repeat_application_activation() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    context.input.physical_focus = true;
    context.input.query_focus = true;
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[first, 5], &mut memory, call)
        .unwrap();
    let (_, initial) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(
        initial.iter().filter(|message| message[1] == 0x1c).count(),
        2
    );
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[second, 5], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert!(messages.contains(&[first, 6, 0, second]));
    assert!(messages.contains(&[second, 6, 1, first]));
    assert!(messages.iter().all(|message| message[1] != 0x1c));
    assert_eq!(context.windows().active().unwrap().get(), second);
}

#[test]
fn child_keyboard_focus_does_not_deactivate_its_top_level_application() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    let mut outcome = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x4000_0000,
                0,
                0,
                10,
                10,
                window,
                0,
                call.instance,
                0,
            ],
            &mut memory,
            call,
        )
        .unwrap();
    let child = loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        &mut memory,
                    )
                    .unwrap();
            }
            User32Outcome32::Return(handle) => break handle,
            _ => panic!("child creation cannot wait"),
        }
    };
    assert_ne!(child, 0);
    context.input.physical_focus = true;
    context.input.query_focus = true;
    let outcome = context
        .prepare_arguments(User32Api32::ShowWindow, &[window, 5], &mut memory, call)
        .unwrap();
    let (_, initial) = drain_show(&mut context, &mut memory, outcome);
    assert_eq!(
        initial.iter().filter(|message| message[1] == 0x1c).count(),
        1
    );
    let outcome = context
        .prepare_arguments(User32Api32::SetFocus, &[child], &mut memory, call)
        .unwrap();
    let (_, messages) = drain_show(&mut context, &mut memory, outcome);
    assert!(messages.contains(&[window, 8, child, 0]));
    assert!(messages.contains(&[child, 7, window, 0]));
    assert!(messages.iter().all(|message| message[1] != 0x1c));
    assert_eq!(context.windows().active().unwrap().get(), window);
    assert_eq!(context.windows().focus().unwrap().get(), child);
}
