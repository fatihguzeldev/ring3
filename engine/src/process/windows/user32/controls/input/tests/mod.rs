use super::*;
use crate::{
    input::{
        InputKind,
        source::{SourceEvent32, SourceKind32},
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::tests::{create, fixture, register},
    windows::{
        user32::{Point32, User32Api32},
        window::WindowError32,
    },
};
const PROC_A: u32 = 0x71000;
const PROC_W: u32 = 0x71020;
#[derive(Default)]
struct Observed {
    commands: Vec<[u32; 5]>,
    callbacks: usize,
}
fn execute(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    args: &[u32],
) -> Result<User32Outcome32, E> {
    let window = context.owned(args[0], call.thread_id)?;
    let kind = context
        .state
        .standard_kind(context.state.record(window)?.class_atom)
        .unwrap();
    match context.control_input_message(
        window,
        kind,
        args,
        call,
        InputMetrics32 {
            height: 16,
            caret: Some(0),
        },
    )? {
        Some(outcome) => Ok(outcome),
        None => context.control_message(args, true, ram, call),
    }
}
fn settle(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    mut outcome: User32Outcome32,
    observed: &mut Observed,
) -> u32 {
    loop {
        match outcome {
            User32Outcome32::Return(value) => return value,
            User32Outcome32::Callback(callback) => {
                observed.callbacks += 1;
                assert!(observed.callbacks < 256);
                let value = if matches!(callback.procedure.0, PROC_A | PROC_W) {
                    let next = execute(context, ram, call, &callback.arguments).unwrap();
                    settle(context, ram, call, next, observed)
                } else {
                    if callback.arguments[1] == 0x111 {
                        let control = WindowHandle32::try_from(callback.arguments[3]).unwrap();
                        let dropped = context.state.control_state(control).unwrap().dropped;
                        observed.commands.push([
                            callback.arguments[0],
                            callback.arguments[1],
                            callback.arguments[2],
                            callback.arguments[3],
                            u32::from(dropped),
                        ]);
                    }
                    u32::from(callback.arguments[1] == 0x81)
                };
                outcome = context.complete(callback.continuation, value, ram).unwrap();
            }
            User32Outcome32::Wait { .. } => {
                panic!("standard control input cannot add a separate wait")
            }
        }
    }
}
fn setup() -> (
    User32Context32,
    AddressSpace,
    User32CallContext32,
    WindowHandle32,
    u32,
) {
    let (mut context, mut ram, call) = fixture();
    let atom = register(&mut context, &mut ram, call);
    let root = WindowHandle32::try_from(create(&mut context, &mut ram, call, atom)).unwrap();
    let mut description = context.windows().get(root).unwrap().description;
    description.visible = true;
    context
        .state
        .windows_mut()
        .update_description(root, description)
        .unwrap();
    context
        .state
        .windows_mut()
        .set_focus(call.thread_id, Some(root))
        .unwrap();
    while context.state.windows_mut().pop_message().is_some() {}
    context.builtin_procedures = Some([GuestAddress(PROC_A), GuestAddress(PROC_W)]);
    context
        .state
        .install_standard_classes(GuestAddress(PROC_W))
        .unwrap();
    context.input.physical_focus = true;
    context.input.focus = root.get();
    (context, ram, call, root, atom)
}
fn child(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    root: WindowHandle32,
    kind: ControlKind32,
    id: u32,
    style: u32,
    y: u32,
    height: u32,
) -> WindowHandle32 {
    let atom = context.state.standard_atom(kind);
    let outcome = context
        .prepare_arguments(
            User32Api32::CreateWindowExW,
            &[
                0,
                u32::from(atom),
                0,
                0x5000_0000 | style,
                20,
                y,
                100,
                height,
                root.get(),
                id,
                call.instance,
                0,
            ],
            ram,
            call,
        )
        .unwrap();
    WindowHandle32::try_from(settle(
        context,
        ram,
        call,
        outcome,
        &mut Observed::default(),
    ))
    .unwrap()
}
fn send(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    args: &[u32],
    observed: &mut Observed,
) -> u32 {
    let outcome = execute(context, ram, call, args).unwrap();
    settle(context, ram, call, outcome, observed)
}
fn mouse(
    context: &mut User32Context32,
    ram: &mut AddressSpace,
    call: User32CallContext32,
    point: Point32,
    pressed: bool,
    observed: &mut Observed,
) -> u32 {
    context.input.focus = context.windows().focus().map_or(0, WindowHandle32::get);
    context.input.capture = context.windows().capture().map_or(0, WindowHandle32::get);
    context.input.point = point;
    context
        .deliver_input(
            SourceEvent32 {
                sequence: 1,
                timestamp_ns: 1_000_000,
                kind: SourceKind32::Physical(InputKind::Button { button: 0, pressed }),
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let message = if pressed { 0x201 } else { 0x202 };
    let (index, queued) = context
        .state
        .find_message(call.thread_id, 0, message, message)
        .unwrap()
        .expect("physical source must reach the canonical target queue");
    context.state.remove_message(index);
    let outcome = execute(
        context,
        ram,
        call,
        &[queued.window, queued.message, queued.wparam, queued.lparam],
    )
    .unwrap();
    settle(context, ram, call, outcome, observed);
    queued.window
}
fn pt(x: i16, y: i16) -> u32 {
    u32::from(x as u16) | (u32::from(y as u16) << 16)
}
#[test]
fn physical_button_targets_child_tracks_capture_and_emits_one_real_parent_notification() {
    let (mut context, mut ram, call, root, _) = setup();
    let button = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::Button,
        44,
        3,
        30,
        24,
    );
    let mut observed = Observed::default();
    assert_eq!(
        mouse(
            &mut context,
            &mut ram,
            call,
            Point32 { x: 35, y: 55 },
            true,
            &mut observed
        ),
        button.get()
    );
    assert_eq!(context.windows().capture(), Some(button));
    assert_eq!(context.windows().focus(), Some(button));
    assert!(context.state.control_state(button).unwrap().pressed);
    assert!(observed.commands.is_empty());
    assert_eq!(
        mouse(
            &mut context,
            &mut ram,
            call,
            Point32 { x: 35, y: 55 },
            false,
            &mut observed
        ),
        button.get()
    );
    assert_eq!(context.windows().capture(), None);
    assert_eq!(context.state.control_state(button).unwrap().check, 1);
    assert_eq!(
        observed.commands,
        vec![[root.get(), 0x111, 44, button.get(), 0]]
    );
}
#[test]
fn button_drag_outside_and_lost_focus_cancel_while_space_clicks_only_once() {
    let (mut context, mut ram, call, root, _) = setup();
    let button = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::Button,
        44,
        3,
        30,
        24,
    );
    let mut observed = Observed::default();
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x201, 1, pt(4, 4)],
        &mut observed,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x200, 1, pt(-4, 4)],
        &mut observed,
    );
    assert!(!context.state.control_state(button).unwrap().pressed);
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x202, 0, pt(-4, 4)],
        &mut observed,
    );
    assert!(observed.commands.is_empty());
    assert_eq!(context.windows().capture(), None);
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x100, 32, 0],
        &mut observed,
    );
    assert!(context.state.control_state(button).unwrap().pressed);
    assert_eq!(context.windows().capture(), Some(button));
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x101, 32, 0],
        &mut observed,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x101, 32, 0],
        &mut observed,
    );
    assert_eq!(observed.commands.len(), 1);
    assert_eq!(context.state.control_state(button).unwrap().check, 1);
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x201, 1, pt(4, 4)],
        &mut observed,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 8, root.get(), 0],
        &mut observed,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x202, 0, pt(4, 4)],
        &mut observed,
    );
    assert_eq!(observed.commands.len(), 1);
    assert_eq!(context.windows().capture(), None);
}
#[test]
fn physical_list_row_uses_owned_items_and_sends_selection_from_the_actual_control() {
    let (mut context, mut ram, call, root, _) = setup();
    let list = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::ListBox,
        45,
        1,
        70,
        80,
    );
    ram.write(GuestAddress(0x10100), &[b'a', 0, 0, 0]).unwrap();
    send(
        &mut context,
        &mut ram,
        call,
        &[list.get(), 0x180, 0, 0x10100],
        &mut Observed::default(),
    );
    ram.write(GuestAddress(0x10100), &[b'b', 0, 0, 0]).unwrap();
    send(
        &mut context,
        &mut ram,
        call,
        &[list.get(), 0x180, 0, 0x10100],
        &mut Observed::default(),
    );
    let mut observed = Observed::default();
    mouse(
        &mut context,
        &mut ram,
        call,
        Point32 { x: 35, y: 110 },
        true,
        &mut observed,
    );
    mouse(
        &mut context,
        &mut ram,
        call,
        Point32 { x: 35, y: 110 },
        false,
        &mut observed,
    );
    assert_eq!(context.state.control_state(list).unwrap().selected, Some(1));
    assert!(
        observed
            .commands
            .contains(&[root.get(), 0x111, 45 | (1 << 16), list.get(), 0])
    );
    assert_eq!(context.windows().focus(), Some(list));
    assert_eq!(context.windows().capture(), None);
}
#[test]
fn combo_drop_notifies_before_showing_and_owns_one_real_list_with_shared_item_authority() {
    let (mut context, mut ram, call, root, _) = setup();
    let combo = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::ComboBox,
        46,
        3,
        100,
        100,
    );
    ram.write(GuestAddress(0x10100), &[b'a', 0, 0, 0]).unwrap();
    send(
        &mut context,
        &mut ram,
        call,
        &[combo.get(), 0x143, 0, 0x10100],
        &mut Observed::default(),
    );
    ram.write(GuestAddress(0x10100), &[b'b', 0, 0, 0]).unwrap();
    send(
        &mut context,
        &mut ram,
        call,
        &[combo.get(), 0x143, 0, 0x10100],
        &mut Observed::default(),
    );
    let before = context.windows().live_windows();
    let mut observed = Observed::default();
    assert_eq!(
        send(
            &mut context,
            &mut ram,
            call,
            &[combo.get(), 0x14f, 1, 0],
            &mut observed
        ),
        1
    );
    assert_eq!(
        observed.commands,
        vec![[root.get(), 0x111, 46 | (7 << 16), combo.get(), 0]]
    );
    let popup = context.state.control_state(combo).unwrap().popup.unwrap();
    assert!(context.windows().get(popup).unwrap().description.visible);
    assert_eq!(context.windows().live_windows(), before + 1);
    assert_eq!(
        context.state.control_state(combo).unwrap().items.as_ptr(),
        context.state.control_state(popup).unwrap().items.as_ptr()
    );
    assert_eq!(context.windows().focus(), Some(root));
    let origin = context.state.record(popup).unwrap().client_origin;
    mouse(
        &mut context,
        &mut ram,
        call,
        Point32 {
            x: origin.x + 4,
            y: origin.y + 19,
        },
        true,
        &mut observed,
    );
    mouse(
        &mut context,
        &mut ram,
        call,
        Point32 {
            x: origin.x + 4,
            y: origin.y + 19,
        },
        false,
        &mut observed,
    );
    assert_eq!(
        context.state.control_state(combo).unwrap().selected,
        Some(1)
    );
    assert!(!context.windows().get(popup).unwrap().description.visible);
    assert!(
        observed
            .commands
            .iter()
            .any(|row| row[2] == (46 | (9 << 16)))
    );
    assert!(
        observed
            .commands
            .iter()
            .any(|row| row[2] == (46 | (8 << 16)))
    );
    for _ in 0..20 {
        send(
            &mut context,
            &mut ram,
            call,
            &[combo.get(), 0x14f, 1, 0],
            &mut Observed::default(),
        );
        send(
            &mut context,
            &mut ram,
            call,
            &[combo.get(), 0x14f, 0, 0],
            &mut Observed::default(),
        );
    }
    assert_eq!(context.windows().live_windows(), before + 1);
    assert_eq!(
        context.state.control_state(combo).unwrap().popup,
        Some(popup)
    );
}
#[test]
fn combo_capacity_failure_preserves_all_window_control_and_guest_state() {
    let (mut context, mut ram, call, root, atom) = setup();
    let combo = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::ComboBox,
        46,
        3,
        100,
        100,
    );
    for _ in 0..6 {
        create(&mut context, &mut ram, call, atom);
    }
    let version = ram.transaction_version();
    let count = context.windows().live_windows();
    let focus = context.windows().focus();
    assert!(matches!(
        execute(&mut context, &mut ram, call, &[combo.get(), 0x14f, 1, 0]),
        Err(E::Window(WindowError32::Capacity))
    ));
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(context.windows().live_windows(), count);
    assert_eq!(context.windows().focus(), focus);
    assert_eq!(context.windows().capture(), None);
    let state = context.state.control_state(combo).unwrap();
    assert!(!state.dropped && !state.opening && state.popup.is_none());
    assert!(context.pending.is_empty());
}

#[test]
fn disabled_button_procedure_accepts_explicit_messages_but_physical_hit_testing_excludes_it() {
    let (mut context, mut ram, call, root, _) = setup();
    let button = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::Button,
        44,
        3,
        30,
        24,
    );
    let mut description = context.windows().get(button).unwrap().description;
    description.enabled = false;
    context
        .state
        .windows_mut()
        .update_description(button, description)
        .unwrap();
    let mut observed = Observed::default();
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x201, 1, pt(4, 4)],
        &mut observed,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[button.get(), 0x202, 0, pt(4, 4)],
        &mut observed,
    );
    assert_eq!(context.state.control_state(button).unwrap().check, 1);
    assert_eq!(
        observed.commands,
        vec![[root.get(), 0x111, 44, button.get(), 0]]
    );
    assert_eq!(context.windows().focus(), Some(root));
    context.input.focus = root.get();
    context.input.capture = 0;
    context.input.point = Point32 { x: 35, y: 55 };
    context
        .deliver_input(
            SourceEvent32 {
                sequence: 1,
                timestamp_ns: 1_000_000,
                kind: SourceKind32::Physical(InputKind::Button {
                    button: 0,
                    pressed: true,
                }),
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let (_, queued) = context
        .state
        .find_message(call.thread_id, 0, 0x201, 0x201)
        .unwrap()
        .unwrap();
    assert_eq!(queued.window, root.get());
}
#[test]
fn accepted_dropdown_notification_retries_capture_capacity_without_replaying_parent_or_showing_early()
 {
    let (mut context, mut ram, call, root, _) = setup();
    let combo = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::ComboBox,
        46,
        3,
        100,
        100,
    );
    context
        .state
        .windows_mut()
        .set_focus(call.thread_id, Some(combo))
        .unwrap();
    while context.state.windows_mut().pop_message().is_some() {}
    context
        .state
        .windows_mut()
        .set_capture(call.thread_id, Some(root))
        .unwrap();
    let outcome = execute(
        &mut context,
        &mut ram,
        call,
        &[combo.get(), 0x201, 1, pt(4, 4)],
    )
    .unwrap();
    let User32Outcome32::Callback(notification) = outcome else {
        panic!("drop must notify its real parent before visibility");
    };
    assert_eq!(
        notification.arguments,
        [root.get(), 0x111, 46 | (7 << 16), combo.get()]
    );
    let popup = context.state.control_state(combo).unwrap().popup.unwrap();
    for target in [root, combo, root, combo] {
        context
            .state
            .windows_mut()
            .set_focus(call.thread_id, Some(target))
            .unwrap();
    }
    let version = ram.transaction_version();
    assert_eq!(
        context.complete(notification.continuation, 0, &mut ram),
        Err(E::Window(WindowError32::QueueFull))
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(context.windows().capture(), Some(root));
    assert!(!context.windows().get(popup).unwrap().description.visible);
    assert!(context.state.control_state(combo).unwrap().opening);
    assert_eq!(
        context.pending.last().unwrap().token,
        notification.continuation
    );
    while context.state.windows_mut().pop_message().is_some() {}
    let retry = context
        .complete(notification.continuation, 0, &mut ram)
        .unwrap();
    let mut observed = Observed::default();
    assert_eq!(
        settle(&mut context, &mut ram, call, retry, &mut observed),
        0
    );
    assert!(observed.commands.is_empty());
    assert_eq!(context.windows().capture(), Some(popup));
    assert!(context.windows().get(popup).unwrap().description.visible);
    assert!(context.state.control_state(combo).unwrap().dropped);
}
#[test]
fn destroying_a_combo_retires_its_canonical_popup_and_its_single_item_owner() {
    let (mut context, mut ram, call, root, _) = setup();
    let combo = child(
        &mut context,
        &mut ram,
        call,
        root,
        ControlKind32::ComboBox,
        46,
        3,
        100,
        100,
    );
    send(
        &mut context,
        &mut ram,
        call,
        &[combo.get(), 0x14f, 1, 0],
        &mut Observed::default(),
    );
    let popup = context.state.control_state(combo).unwrap().popup.unwrap();
    let count = context.windows().live_windows();
    let outcome = context.destroy_window(combo.get(), call).unwrap();
    assert_eq!(
        settle(
            &mut context,
            &mut ram,
            call,
            outcome,
            &mut Observed::default()
        ),
        1
    );
    assert!(context.windows().get(combo).is_err() && context.windows().get(popup).is_err());
    assert_eq!(context.windows().live_windows(), count - 2);
}
