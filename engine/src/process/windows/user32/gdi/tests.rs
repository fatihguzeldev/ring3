use super::*;
use crate::{
    memory::GuestAddress,
    process::windows::gdi::{GdiConfig32, GdiRuntime32},
    process::windows::user32::{
        User32CallContext32,
        tests::{create, fixture, register},
    },
    windows::{
        gdi::Region32,
        user32::{Point32, User32Api32, dialogs::WindowDialog32},
    },
};

fn dirty(context: &mut User32Context32, window: WindowHandle32) -> Region32 {
    let mut description = context.windows().get(window).unwrap().description;
    description.visible = true;
    context
        .state
        .windows_mut()
        .update_description(window, description)
        .unwrap();
    let region = Region32::rectangle(Rect32 {
        left: 0,
        top: 0,
        right: 40,
        bottom: 30,
    });
    context
        .gdi_set_update_region(window, region.clone(), true)
        .unwrap();
    region
}

fn paint_available(context: &User32Context32, call: User32CallContext32) -> bool {
    context
        .state
        .paint_message(call.thread_id, 0, 0, 0, u32::MAX, 0, Point32::default())
        .unwrap()
        .is_some()
}

#[test]
fn default_paint_validates_the_region_and_allows_later_invalidation() {
    for api in [User32Api32::DefWindowProcA, User32Api32::DefWindowProcW] {
        let (mut context, mut memory, call) = fixture();
        let atom = register(&mut context, &mut memory, call);
        let hwnd = create(&mut context, &mut memory, call, atom);
        let window = WindowHandle32::try_from(hwnd).unwrap();
        let region = dirty(&mut context, window);
        assert!(paint_available(&context, call));
        assert_eq!(
            context
                .prepare_arguments(api, &[hwnd, 0xf, 0, 0], &mut memory, call)
                .unwrap(),
            User32Outcome32::Return(0)
        );
        let (update, erase) = context.gdi_update_region(window).unwrap();
        assert_eq!(update.complexity(), 1);
        assert!(!erase);
        assert!(!paint_available(&context, call));
        assert_eq!(context.state.paint_status(call.thread_id), (0, 0));
        context
            .gdi_set_update_region(window, region, false)
            .unwrap();
        assert!(paint_available(&context, call));
    }
}

#[test]
fn unhandled_dialog_paint_reaches_the_registered_default_and_finishes_its_callback_once() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let hwnd = create(&mut context, &mut memory, call, atom);
    let window = WindowHandle32::try_from(hwnd).unwrap();
    dirty(&mut context, window);
    context
        .state
        .set_dialog(
            window,
            WindowDialog32 {
                base: Point32 { x: 8, y: 16 },
                font: 0,
                modal: false,
                ended: None,
            },
        )
        .unwrap();
    context.state.set_extra(window, 4, 0x70100).unwrap();
    context.resource_dialogs.defaults = Some([GuestAddress(0x70200); 2]);
    let User32Outcome32::Callback(dialog) = context
        .prepare_arguments(
            User32Api32::DefDlgProcA,
            &[hwnd, 0xf, 0, 0],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!("dialog procedure callback required");
    };
    assert_eq!(dialog.procedure, GuestAddress(0x70100));
    let User32Outcome32::Callback(default) = context
        .complete(dialog.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!("real default callback required");
    };
    assert_eq!(default.procedure, GuestAddress(0x70200));
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::DefWindowProcA,
                &default.arguments,
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        context
            .complete(default.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert!(!paint_available(&context, call));
    assert!(context.pending.is_empty());
}

#[test]
fn default_erase_uses_the_real_window_callback_and_preserves_its_new_invalidation() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let hwnd = create(&mut context, &mut memory, call, atom);
    let window = WindowHandle32::try_from(hwnd).unwrap();
    let region = dirty(&mut context, window);
    let mut gdi = GdiRuntime32::new(GdiConfig32::default()).unwrap();
    context
        .gdi_check_begin_paint(window, true, call.thread_id)
        .unwrap();
    let dc = gdi
        .begin_query_erase(
            context.gdi_window(hwnd, false).unwrap(),
            call.thread_id,
            region,
        )
        .unwrap();
    let User32Outcome32::Callback(callback) = context
        .gdi_begin_default_paint_erase(window, dc, call.thread_id)
        .unwrap()
    else {
        panic!("real erase callback required");
    };
    assert_eq!(callback.procedure, GuestAddress(0x70000));
    assert_eq!(callback.arguments, [hwnd, 0x14, dc, 0]);
    assert_eq!(context.gdi_update_region(window).unwrap().0.complexity(), 1);
    let next = Region32::rectangle(Rect32 {
        left: 4,
        top: 5,
        right: 9,
        bottom: 10,
    });
    context
        .gdi_set_update_region(window, next.clone(), false)
        .unwrap();
    assert_eq!(
        context
            .complete_with_gdi(callback.continuation, 1, &mut memory, Some(&mut gdi))
            .unwrap(),
        User32Outcome32::Return(0)
    );
    let (update, erase) = context.gdi_update_region(window).unwrap();
    assert_eq!(update.bounds(), next.bounds());
    assert!(!erase);
    assert!(paint_available(&context, call));
    assert!(
        gdi.prepare_query_erase_finish(hwnd, dc, call.thread_id)
            .is_err()
    );
    assert!(context.pending.is_empty());
}

#[test]
fn dialog_without_an_application_proc_delegates_default_paint_and_erase() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let hwnd = create(&mut context, &mut memory, call, atom);
    let window = WindowHandle32::try_from(hwnd).unwrap();
    context
        .state
        .set_dialog(
            window,
            WindowDialog32 {
                base: Point32 { x: 8, y: 16 },
                font: 0,
                modal: false,
                ended: None,
            },
        )
        .unwrap();
    context.resource_dialogs.defaults = Some([GuestAddress(0x70200), GuestAddress(0x70300)]);
    for (api, procedure) in [
        (User32Api32::DefDlgProcA, 0x70200),
        (User32Api32::DefDlgProcW, 0x70300),
    ] {
        for message in [0x0f, 0x14] {
            let args = [hwnd, message, 0, 0];
            let User32Outcome32::Callback(default) = context
                .prepare_arguments(api, &args, &mut memory, call)
                .unwrap()
            else {
                panic!("registered default callback required");
            };
            assert_eq!(default.procedure, GuestAddress(procedure));
            assert_eq!(default.arguments, args);
            assert_eq!(
                context
                    .complete(default.continuation, 0, &mut memory)
                    .unwrap(),
                User32Outcome32::Return(0)
            );
        }
    }
    assert!(context.pending.is_empty());
}
