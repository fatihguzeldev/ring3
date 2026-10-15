use super::*;

fn related(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    atom: u32,
    parent: u32,
    style: u32,
) -> u32 {
    let mut outcome = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[0, atom, 0, style, 0, 0, 32, 32, parent, 0, call.instance, 0],
            memory,
            call,
        )
        .unwrap();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        memory,
                    )
                    .unwrap()
            }
            User32Outcome32::Return(handle) => {
                assert_ne!(handle, 0);
                return handle;
            }
            _ => panic!(),
        }
    }
}

#[test]
fn owned_popup_finishes_first_then_child_destroy_preorder_precedes_ncdestroy_postorder() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let parent = create(&mut context, &mut memory, call, atom);
    let child1 = related(&mut context, &mut memory, call, atom, parent, 0x40000000);
    let grandchild = related(&mut context, &mut memory, call, atom, child1, 0x40000000);
    let child2 = related(&mut context, &mut memory, call, atom, parent, 0x40000000);
    let popup = related(&mut context, &mut memory, call, atom, parent, 0x80000000);
    let User32Outcome32::Callback(focus) = context
        .prepare_arguments(User32Api32::SetFocus, &[grandchild], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    context
        .complete(focus.continuation, 0, &mut memory)
        .unwrap();
    context
        .prepare_arguments(User32Api32::SetCapture, &[popup], &mut memory, call)
        .unwrap();
    let mut outcome = context
        .prepare_arguments(User32Api32::DestroyWindow, &[parent], &mut memory, call)
        .unwrap();
    let mut messages = Vec::new();
    let mut capture_cleanup = false;
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                let [window, message, _, _] = callback.arguments;
                assert!(
                    context
                        .windows()
                        .get(WindowHandle32::try_from(window).unwrap())
                        .is_ok()
                );
                if message == 2 || message == 0x82 {
                    messages.push((window, message));
                }
                if window == popup && message == 0x82 {
                    assert_eq!(context.windows().capture().unwrap().get(), popup);
                }
                if message == 0x215 {
                    assert_eq!(window, popup);
                    assert_eq!(context.windows().capture(), None);
                    capture_cleanup = true;
                }
                if message == 0x82 {
                    assert_eq!(
                        context
                            .prepare_arguments(
                                User32Api32::UnregisterClassA,
                                &[atom, call.instance],
                                &mut memory,
                                call
                            )
                            .unwrap(),
                        User32Outcome32::Return(0)
                    );
                    assert_eq!(context.take_last_error(), Some(1412));
                }
                outcome = context
                    .complete(callback.continuation, 0, &mut memory)
                    .unwrap();
            }
            User32Outcome32::Return(result) => {
                assert_eq!(result, 1);
                break;
            }
            _ => panic!(),
        }
    }
    assert_eq!(
        messages,
        [
            (popup, 2),
            (popup, 0x82),
            (parent, 2),
            (child1, 2),
            (grandchild, 2),
            (child2, 2),
            (grandchild, 0x82),
            (child1, 0x82),
            (child2, 0x82),
            (parent, 0x82)
        ]
    );
    assert!(capture_cleanup);
    assert_eq!(context.windows().live_windows(), 0);
    assert_eq!(context.windows().focus(), None);
    assert_eq!(context.windows().capture(), None);
    assert!(context.destructions.is_empty());
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::UnregisterClassA,
                &[atom, call.instance],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
}

#[test]
fn reentrant_child_destroy_retires_its_subtree_once_and_parent_skips_stale_plan_entries() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let parent = create(&mut context, &mut memory, call, atom);
    let child = related(&mut context, &mut memory, call, atom, parent, 0x40000000);
    let grandchild = related(&mut context, &mut memory, call, atom, child, 0x40000000);
    let User32Outcome32::Callback(outer) = context
        .prepare_arguments(User32Api32::DestroyWindow, &[parent], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(outer.arguments[..2], [parent, 2]);
    let mut outcome = context
        .prepare_arguments(User32Api32::DestroyWindow, &[child], &mut memory, call)
        .unwrap();
    assert_eq!(
        context.complete(outer.continuation, 0, &mut memory),
        Err(User32Error32::InvalidToken)
    );
    let mut messages = Vec::new();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                messages.push((callback.arguments[0], callback.arguments[1]));
                outcome = context
                    .complete(callback.continuation, 0, &mut memory)
                    .unwrap();
            }
            User32Outcome32::Return(result) => {
                assert_eq!(result, 1);
                break;
            }
            _ => panic!(),
        }
    }
    assert_eq!(
        messages,
        [
            (child, 2),
            (grandchild, 2),
            (grandchild, 0x82),
            (child, 0x82)
        ]
    );
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(child).unwrap())
            .is_err()
    );
    let User32Outcome32::Callback(last) = context
        .complete(outer.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(last.arguments[..2], [parent, 0x82]);
    assert_eq!(
        context.complete(last.continuation, 0, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    assert!(context.pending.is_empty());
    assert!(context.destructions.is_empty());
    let next = create(&mut context, &mut memory, call, atom);
    assert!(next > grandchild);
}

#[test]
fn destroying_focused_child_transfers_focus_to_surviving_parent_before_destroy_message() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let parent = create(&mut context, &mut memory, call, atom);
    let child = related(&mut context, &mut memory, call, atom, parent, 0x40000000);
    let User32Outcome32::Callback(focus) = context
        .prepare_arguments(User32Api32::SetFocus, &[child], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    context
        .complete(focus.continuation, 0, &mut memory)
        .unwrap();
    let User32Outcome32::Callback(kill) = context
        .prepare_arguments(User32Api32::DestroyWindow, &[child], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(kill.arguments, [child, 8, parent, 0]);
    assert_eq!(context.windows().focus().unwrap().get(), parent);
    let User32Outcome32::Callback(focus) =
        context.complete(kill.continuation, 0, &mut memory).unwrap()
    else {
        panic!()
    };
    assert_eq!(focus.arguments, [parent, 7, child, 0]);
    let User32Outcome32::Callback(destroy) = context
        .complete(focus.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(destroy.arguments[..2], [child, 2]);
    let User32Outcome32::Callback(nc) = context
        .complete(destroy.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(nc.arguments[..2], [child, 0x82]);
    context.complete(nc.continuation, 0, &mut memory).unwrap();
    assert_eq!(context.windows().focus().unwrap().get(), parent);
    assert_eq!(context.windows().live_windows(), 1);
}
