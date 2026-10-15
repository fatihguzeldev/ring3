use super::*;

#[test]
fn creation_delivers_real_callbacks_and_publishes_create_struct_before_valid_hwnd() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let before = memory.mapped_pages();
    let User32Outcome32::Callback(first) = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x80000000,
                10,
                20,
                640,
                480,
                0,
                0,
                call.instance,
                0x1234,
            ],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    let handle = WindowHandle32::try_from(first.arguments[0]).unwrap();
    assert!(context.windows().get(handle).is_ok());
    assert_eq!(first.procedure, GuestAddress(0x70000));
    assert_eq!(first.arguments[1], 0x81);
    let create_struct: [u32; 12] =
        super::super::memory::words(&memory, first.arguments[3]).unwrap();
    assert_eq!(
        create_struct,
        [
            0x1234,
            call.instance,
            0,
            0,
            480,
            640,
            20,
            10,
            0x80000000,
            0,
            atom,
            0
        ]
    );
    let User32Outcome32::Callback(size) = context
        .complete(first.continuation, 1, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(size.arguments[1], 0x83);
    super::super::memory::write_rect(
        &mut memory,
        size.arguments[3],
        crate::windows::user32::Rect32 {
            left: 12,
            top: 24,
            right: 648,
            bottom: 496,
        },
    )
    .unwrap();
    let User32Outcome32::Callback(create) =
        context.complete(size.continuation, 0, &mut memory).unwrap()
    else {
        panic!()
    };
    assert_eq!(create.arguments[1], 1);
    assert_eq!(
        context
            .windows()
            .get(handle)
            .unwrap()
            .description
            .extent
            .width,
        636
    );
    let User32Outcome32::Callback(size) = context
        .complete(create.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(size.arguments[1], 5);
    let User32Outcome32::Callback(moved) =
        context.complete(size.continuation, 0, &mut memory).unwrap()
    else {
        panic!()
    };
    assert_eq!(moved.arguments[1], 3);
    assert_eq!(
        context
            .complete(moved.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(handle.get())
    );
    assert_eq!(memory.mapped_pages(), before);
    assert_eq!(
        context.complete(first.continuation, 1, &mut memory),
        Err(User32Error32::InvalidToken)
    );
}

#[test]
fn failed_nccreate_delivers_ncdestroy_and_reclaims_identity_without_reuse() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let before = memory.mapped_pages();
    let User32Outcome32::Callback(first) = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x80000000,
                0,
                0,
                320,
                200,
                0,
                0,
                call.instance,
                0,
            ],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    let User32Outcome32::Callback(destroy) = context
        .complete(first.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(destroy.arguments[1], 0x82);
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(first.arguments[0]).unwrap())
            .is_ok()
    );
    assert_eq!(
        context
            .complete(destroy.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(memory.mapped_pages(), before);
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(first.arguments[0]).unwrap())
            .is_err()
    );
    let next = create(&mut context, &mut memory, call, atom);
    assert!(next > first.arguments[0]);
}

#[test]
fn nested_focus_callback_preserves_parent_continuation_and_destruction_identity_until_ncdestroy() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second = create(&mut context, &mut memory, call, atom);
    let User32Outcome32::Callback(focus) = context
        .prepare_arguments(User32Api32::SetFocus, &[first], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(focus.arguments, [first, 7, 0, 0]);
    let User32Outcome32::Callback(inner) = context
        .prepare_arguments(User32Api32::SetFocus, &[second], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(inner.arguments, [first, 8, second, 0]);
    assert_eq!(
        context.complete(focus.continuation, 0, &mut memory),
        Err(User32Error32::InvalidToken)
    );
    let User32Outcome32::Callback(next) = context
        .complete(inner.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(next.arguments, [second, 7, first, 0]);
    assert_eq!(
        context.complete(next.continuation, 0, &mut memory).unwrap(),
        User32Outcome32::Return(first)
    );
    assert_eq!(
        context
            .complete(focus.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    let mut outcome = context
        .prepare_arguments(User32Api32::DestroyWindow, &[second], &mut memory, call)
        .unwrap();
    let mut messages = Vec::new();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                assert!(
                    context
                        .windows()
                        .get(WindowHandle32::try_from(second).unwrap())
                        .is_ok()
                );
                messages.push(callback.arguments[1]);
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
    assert_eq!(messages, [8, 2, 0x82]);
    assert_eq!(context.windows().focus(), None);
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(second).unwrap())
            .is_err()
    );
}

#[test]
fn windowpos_changing_can_modify_size_before_nonclient_and_changed_callbacks() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    let before = memory.mapped_pages();
    let User32Outcome32::Callback(changing) = context
        .prepare_arguments(
            User32Api32::MoveWindow,
            &[window, 30, 40, 800, 600, 1],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(changing.arguments[1], 0x46);
    assert_eq!(
        context
            .windows()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .extent
            .width,
        640
    );
    memory
        .write(
            GuestAddress(changing.arguments[3] + 16),
            &320u32.to_le_bytes(),
        )
        .unwrap();
    let User32Outcome32::Callback(nc) = context
        .complete(changing.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(nc.arguments[1], 0x83);
    let User32Outcome32::Callback(changed) =
        context.complete(nc.continuation, 0, &mut memory).unwrap()
    else {
        panic!()
    };
    assert_eq!(changed.arguments[1], 0x47);
    assert_eq!(
        context
            .windows()
            .get(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .description
            .extent
            .width,
        320
    );
    let User32Outcome32::Callback(moved) = context
        .prepare_arguments(
            User32Api32::DefWindowProcA,
            &changed.arguments,
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(moved.arguments[1], 3);
    let User32Outcome32::Callback(sized) = context
        .complete(moved.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(sized.arguments[1], 5);
    assert_eq!(
        context
            .complete(sized.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        context
            .complete(changed.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(memory.mapped_pages(), before);
}

#[test]
fn invalid_class_and_scratch_capacity_leave_window_and_mapping_authority_unchanged() {
    let (mut context, mut memory, call) = fixture();
    let before = memory.mapped_pages();
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::CreateWindowExA,
                &[0, 0xcfff, 0, 0, 0, 0, 1, 1, 0, 0, call.instance, 0],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(context.take_last_error(), Some(1407));
    assert_eq!(context.windows().live_windows(), 0);
    let atom = register(&mut context, &mut memory, call);
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x40000), 8).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mapped = memory.mapped_pages();
    assert!(matches!(
        context.prepare_arguments(
            User32Api32::CreateWindowExA,
            &[0, atom, 0, 0, 0, 0, 1, 1, 0, 0, call.instance, 0],
            &mut memory,
            call
        ),
        Err(User32Error32::Memory(crate::memory::MemoryError::Capacity))
    ));
    assert_eq!(context.windows().live_windows(), 0);
    assert_eq!(memory.mapped_pages(), mapped);
    assert_eq!(before, 2);
}

#[test]
fn destroying_window_inside_create_callback_retires_outer_creation_and_scratch() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let before = memory.mapped_pages();
    let User32Outcome32::Callback(nccreate) = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x80000000,
                0,
                0,
                320,
                200,
                0,
                0,
                call.instance,
                0,
            ],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    let User32Outcome32::Callback(ncsize) = context
        .complete(nccreate.continuation, 1, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    let User32Outcome32::Callback(create) = context
        .complete(ncsize.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    let User32Outcome32::Callback(destroy) = context
        .prepare_arguments(
            User32Api32::DestroyWindow,
            &[create.arguments[0]],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(destroy.arguments[1], 2);
    let User32Outcome32::Callback(ncdestroy) = context
        .complete(destroy.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        context
            .complete(ncdestroy.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        context
            .complete(create.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(context.windows().live_windows(), 0);
    assert_eq!(memory.mapped_pages(), before);
    assert!(context.pending.is_empty());
}

#[test]
fn child_screen_rect_and_move_coordinates_follow_parent_client_origin() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let parent = create(&mut context, &mut memory, call, atom);
    let mut outcome = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x40000000,
                3,
                4,
                100,
                50,
                parent,
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
                if callback.arguments[1] == 3 {
                    assert_eq!(callback.arguments[3], 3 | (4 << 16));
                }
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        &mut memory,
                    )
                    .unwrap();
            }
            User32Outcome32::Return(handle) => break handle,
            _ => panic!(),
        }
    };
    context
        .prepare_arguments(
            User32Api32::GetWindowRect,
            &[child, 0x10200],
            &mut memory,
            call,
        )
        .unwrap();
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [13, 24, 113, 74]
    );
    let mut outcome = context
        .prepare_arguments(
            User32Api32::MoveWindow,
            &[parent, 30, 40, 640, 480, 0],
            &mut memory,
            call,
        )
        .unwrap();
    loop {
        match outcome {
            User32Outcome32::Callback(callback) => {
                outcome = context
                    .complete(callback.continuation, 0, &mut memory)
                    .unwrap()
            }
            User32Outcome32::Return(result) => {
                assert_eq!(result, 1);
                break;
            }
            _ => panic!(),
        }
    }
    context
        .prepare_arguments(
            User32Api32::GetWindowRect,
            &[child, 0x10200],
            &mut memory,
            call,
        )
        .unwrap();
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [33, 44, 133, 94]
    );
    assert_eq!(
        context
            .state
            .client_position(WindowHandle32::try_from(child).unwrap())
            .unwrap(),
        crate::windows::user32::Point32 { x: 3, y: 4 }
    );
}
