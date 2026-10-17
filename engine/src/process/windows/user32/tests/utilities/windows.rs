use super::*;
#[test]
fn window_property_lifetime_scroll_clamping_and_enabled_notifications_are_observable() {
    let (mut ctx, mut m, c) = fixture();
    let atom = register(&mut ctx, &mut m, c);
    let h = create(&mut ctx, &mut m, c, atom);
    m.write(GuestAddress(0x10200), b"owner\0").unwrap();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::SetPropA,
            &[h, 0x10200, 0x1234]
        ),
        1
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetPropA, &[h, 0x10200]),
        0x1234
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::RemovePropA, &[h, 0x10200]),
        0x1234
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetPropA, &[h, 0x10200]),
        0
    );
    let q = [28u32, 7, 10, 100, 20, 200, 0];
    m.write(
        GuestAddress(0x10300),
        &q.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::SetScrollInfo,
            &[h, 0, 0x10300, 0]
        ),
        81
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetScrollPos, &[h, 0]),
        81
    );
    let User32Outcome32::Callback(cb) = ctx
        .prepare_arguments(User32Api32::EnableWindow, &[h, 0], &mut m, c)
        .unwrap()
    else {
        panic!("WM_ENABLE callback required")
    };
    assert_eq!(cb.arguments, [h, 0xa, 0, 0]);
    assert_eq!(
        ctx.complete(cb.continuation, 0, &mut m).unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::IsWindowEnabled, &[h]),
        0
    );
}
#[test]
fn unavailable_facilities_fail_explicitly_instead_of_claiming_success() {
    let (mut ctx, mut m, c) = fixture();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::SetWindowsHookExA,
            &[13, 0x80000, 0, 0]
        ),
        0
    );
    assert_eq!(ctx.take_last_error(), Some(120));
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::WinHelpA, &[0, 0, 0, 0]),
        0
    );
    assert_eq!(ctx.take_last_error(), Some(120));
}

#[test]
fn desktop_is_a_canonical_protected_window_with_configured_extent() {
    let (mut ctx, mut m, c) = fixture();
    let rect = crate::windows::user32::Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let window = ctx.install_desktop(GuestAddress(0x70000), rect).unwrap();
    assert_eq!(
        ctx.install_desktop(GuestAddress(0x70000), rect).unwrap(),
        window
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetDesktopWindow, &[]),
        window.get()
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::IsWindow, &[window.get()]),
        1
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetWindowRect,
            &[window.get(), 0x10000]
        ),
        1
    );
    assert_eq!(
        super::super::super::memory::rect(&m, 0x10000).unwrap(),
        rect
    );
    let gdi = ctx.gdi_window(window.get(), false).unwrap();
    assert_eq!((gdi.width, gdi.height), (1920, 1080));
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::DestroyWindow,
            &[window.get()]
        ),
        0
    );
    assert_eq!(ctx.take_last_error(), Some(5));
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::IsWindow, &[window.get()]),
        1
    );
}

#[test]
fn class_long_queries_use_win32_negative_indices_and_process_identity() {
    let (mut ctx, mut m, mut c) = fixture();
    c.process_id = 42;
    let atom = register(&mut ctx, &mut m, c);
    let h = create(&mut ctx, &mut m, c, atom);
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetClassLongA,
            &[h, (-16i32) as u32]
        ),
        c.instance
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetClassLongA,
            &[h, (-20i32) as u32]
        ),
        0
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetClassLongA,
            &[h, (-18i32) as u32]
        ),
        8
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetWindowThreadProcessId,
            &[h, 0x10200]
        ),
        c.thread_id
    );
    let [pid] = super::super::super::memory::words(&m, 0x10200).unwrap();
    assert_eq!(pid, 42);
}

#[test]
fn lazy_desktop_creation_does_not_cover_application_window_or_steal_top_window() {
    let (mut ctx, mut m, c) = fixture();
    let atom = register(&mut ctx, &mut m, c);
    let app = create(&mut ctx, &mut m, c, atom);
    let rect = crate::windows::user32::Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let desktop = ctx.install_desktop(GuestAddress(0x70000), rect).unwrap();
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetTopWindow, &[0]),
        app
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetTopWindow,
            &[desktop.get()]
        ),
        app
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::GetWindow,
            &[desktop.get(), 5]
        ),
        app
    );
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetWindow, &[app, 2]),
        0
    );
}
#[test]
fn placement_rejects_unimplemented_show_transition_instead_of_ignoring_it() {
    let (mut ctx, mut m, c) = fixture();
    let atom = register(&mut ctx, &mut m, c);
    let h = create(&mut ctx, &mut m, c, atom);
    let values = [44u32, 0, 2, 0, 0, 0, 0, 10, 20, 650, 500];
    m.write(
        GuestAddress(0x10300),
        &values
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::SetWindowPlacement,
            &[h, 0x10300]
        ),
        0
    );
    assert_eq!(ctx.take_last_error(), Some(50));
    assert_eq!(call(&mut ctx, &mut m, c, User32Api32::IsIconic, &[h]), 0);
}

#[test]
fn set_active_window_deactivation_delivers_real_nonclient_and_activation_callbacks() {
    let (mut ctx, mut m, c) = fixture();
    let atom = register(&mut ctx, &mut m, c);
    let h = create(&mut ctx, &mut m, c, atom);
    let mut outcome = ctx
        .prepare_arguments(User32Api32::ShowWindow, &[h, 1], &mut m, c)
        .unwrap();
    while let User32Outcome32::Callback(cb) = outcome {
        outcome = ctx.complete(cb.continuation, 1, &mut m).unwrap();
    }
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetActiveWindow, &[]),
        h
    );
    let mut outcome = ctx
        .prepare_arguments(User32Api32::SetActiveWindow, &[0], &mut m, c)
        .unwrap();
    let mut messages = Vec::new();
    while let User32Outcome32::Callback(cb) = outcome {
        messages.push(cb.arguments[1]);
        outcome = ctx.complete(cb.continuation, 1, &mut m).unwrap();
    }
    assert_eq!(messages, vec![0x86, 6, 8]);
    assert_eq!(outcome, User32Outcome32::Return(h));
    assert_eq!(
        call(&mut ctx, &mut m, c, User32Api32::GetActiveWindow, &[]),
        0
    );
}
