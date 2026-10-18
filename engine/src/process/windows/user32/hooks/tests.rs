use super::super::{
    User32CallContext32, User32Context32, User32Outcome32,
    tests::{fixture, register},
};
use super::*;
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    windows::{user32::User32Api32, window::WindowHandle32},
};

#[path = "tests/keyboard.rs"]
mod keyboard;

fn setup() -> (User32Context32, AddressSpace, User32CallContext32, u32) {
    let (mut context, mut memory, call) = fixture();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x70000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let atom = register(&mut context, &mut memory, call);
    (context, memory, call, atom)
}
fn invoke(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    api: User32Api32,
    args: &[u32],
) -> u32 {
    match context.prepare_arguments(api, args, memory, call).unwrap() {
        User32Outcome32::Return(value) => value,
        other => panic!("expected utility return {other:?}"),
    }
}
fn begin(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    atom: u32,
) -> User32Outcome32 {
    context
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
            memory,
            call,
        )
        .unwrap()
}
fn drain(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    mut outcome: User32Outcome32,
) -> (u32, Vec<(u32, u32)>) {
    let mut events = Vec::new();
    for _ in 0..32 {
        match outcome {
            User32Outcome32::Return(value) => return (value, events),
            User32Outcome32::Callback(callback) => {
                events.push((callback.procedure.0, callback.arguments[1]));
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        memory,
                    )
                    .unwrap();
            }
            _ => panic!("creation cannot wait"),
        }
    }
    panic!("bounded creation callbacks");
}
#[test]
fn thread_hook_chain_preserves_lifo_order_after_self_unhook_and_uses_three_arguments() {
    let (mut context, mut memory, call, atom) = setup();
    let first = invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    let second = invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70200, 0, call.thread_id],
    );
    assert_ne!(first, second);
    let User32Outcome32::Callback(outer) = begin(&mut context, &mut memory, call, atom) else {
        panic!("CBT required")
    };
    assert_eq!(
        (outer.procedure.0, outer.count, outer.arguments[0]),
        (0x70200, 3, 3)
    );
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::IsWindow,
            &[outer.arguments[1]]
        ),
        1
    );
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::UnhookWindowsHookEx,
            &[second]
        ),
        1
    );
    let User32Outcome32::Callback(next) = context
        .prepare_arguments(
            User32Api32::CallNextHookEx,
            &[0, 3, outer.arguments[1], outer.arguments[2]],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!("next hook required")
    };
    assert_eq!((next.procedure.0, next.count), (0x70100, 3));
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::CallNextHookEx,
            &[0, 3, outer.arguments[1], outer.arguments[2]]
        ),
        0
    );
    assert_eq!(
        context.complete(next.continuation, 0, &mut memory).unwrap(),
        User32Outcome32::Return(0)
    );
    let after = context
        .complete(outer.continuation, 0, &mut memory)
        .unwrap();
    let (window, events) = drain(&mut context, &mut memory, after);
    assert_eq!(window, outer.arguments[1]);
    assert_eq!(events[0], (0x70000, 0x81));
}
#[test]
fn cbt_mutations_and_subclass_are_committed_before_nonclient_creation() {
    let (mut context, mut memory, call, atom) = setup();
    let hook = invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    let User32Outcome32::Callback(callback) = begin(&mut context, &mut memory, call, atom) else {
        panic!("CBT required")
    };
    let hwnd = callback.arguments[1];
    let [structure, after] = super::super::memory::words(&memory, callback.arguments[2]).unwrap();
    assert_eq!(after, 0);
    for (offset, value) in [(16, 240u32), (20, 320), (24, 70), (28, 50)] {
        memory
            .write(GuestAddress(structure + offset), &value.to_le_bytes())
            .unwrap();
    }
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::SetWindowLongA,
            &[hwnd, (-4i32) as u32, 0x70200]
        ),
        0x70000
    );
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::UnhookWindowsHookEx,
            &[hook]
        ),
        1
    );
    let User32Outcome32::Callback(nccreate) = context
        .complete(callback.continuation, 0, &mut memory)
        .unwrap()
    else {
        panic!("NCCREATE required")
    };
    assert_eq!(
        (
            nccreate.procedure.0,
            nccreate.arguments[1],
            nccreate.arguments[3]
        ),
        (0x70200, 0x81, structure)
    );
    let next = context
        .complete(nccreate.continuation, 1, &mut memory)
        .unwrap();
    let (window, _) = drain(&mut context, &mut memory, next);
    assert_eq!(window, hwnd);
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetWindowRect,
            &[window, 0x10300]
        ),
        1
    );
    assert_eq!(
        super::super::memory::rect(&memory, 0x10300).unwrap(),
        crate::windows::user32::Rect32 {
            left: 50,
            top: 70,
            right: 370,
            bottom: 310
        }
    );
}
#[test]
fn veto_cleans_window_and_scratch_without_create_or_destroy_messages() {
    let (mut context, mut memory, call, atom) = setup();
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    let User32Outcome32::Callback(callback) = begin(&mut context, &mut memory, call, atom) else {
        panic!("CBT required")
    };
    let outcome = context
        .complete(callback.continuation, 1, &mut memory)
        .unwrap();
    let (result, events) = drain(&mut context, &mut memory, outcome);
    assert_eq!(result, 0);
    assert!(
        events
            .iter()
            .all(|(_, message)| !matches!(message, 1 | 2 | 0x81))
    );
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::IsWindow,
            &[callback.arguments[1]]
        ),
        0
    );
    assert_eq!(context.thread_scratch_ranges().count(), 0);
}
#[test]
fn veto_preflights_the_focus_change_for_a_provisional_window() {
    let (mut context, mut memory, call, atom) = setup();
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    let User32Outcome32::Callback(hook) = begin(&mut context, &mut memory, call, atom) else {
        panic!("CBT required")
    };
    let outcome = context
        .prepare_arguments(
            User32Api32::SetFocus,
            &[hook.arguments[1]],
            &mut memory,
            call,
        )
        .unwrap();
    drain(&mut context, &mut memory, outcome);
    assert!(context.windows().focus().is_some());
    assert_eq!(
        context.preview_completion_input_flags(hook.continuation, 1),
        Some((false, false))
    );
    let outcome = context.complete(hook.continuation, 1, &mut memory).unwrap();
    let (result, _) = drain(&mut context, &mut memory, outcome);
    assert_eq!(result, 0);
    assert_eq!(context.windows().focus(), None);
}
#[test]
fn no_op_cbt_preserves_a_foreign_thread_top_level_owner() {
    let (mut context, mut memory, call, atom) = setup();
    let foreign = User32CallContext32 {
        thread_id: call.thread_id + 1,
        ..call
    };
    let outcome = begin(&mut context, &mut memory, foreign, atom);
    let (owner, _) = drain(&mut context, &mut memory, outcome);
    assert_ne!(owner, 0);
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    let User32Outcome32::Callback(hook) = context
        .prepare_arguments(
            User32Api32::CreateWindowExA,
            &[
                0,
                atom,
                0,
                0x8000_0000,
                10,
                20,
                640,
                480,
                owner,
                0,
                call.instance,
                0,
            ],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!("CBT required")
    };
    let outcome = context.complete(hook.continuation, 0, &mut memory).unwrap();
    let (window, events) = drain(&mut context, &mut memory, outcome);
    assert_ne!(window, 0);
    assert_eq!(events[0].1, 0x81);
    assert_eq!(
        context
            .state
            .record(WindowHandle32::try_from(window).unwrap())
            .unwrap()
            .parent,
        Some(WindowHandle32::try_from(owner).unwrap())
    );
}
#[test]
fn unsupported_hook_kinds_and_foreign_threads_fail_while_stale_handles_never_rebind() {
    let (mut context, mut memory, call, _) = setup();
    for (kind, thread) in [(-1i32, call.thread_id), (13, 0), (5, call.thread_id + 1)] {
        assert_eq!(
            invoke(
                &mut context,
                &mut memory,
                call,
                User32Api32::SetWindowsHookExA,
                &[kind as u32, 0x70100, 0, thread]
            ),
            0
        );
        assert_eq!(context.take_last_error(), Some(120));
    }
    let old = invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    context.hooks.retire_thread(call.thread_id);
    let new = invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70100, 0, call.thread_id],
    );
    assert_ne!(old, new);
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::UnhookWindowsHookEx,
            &[old]
        ),
        0
    );
    assert_eq!(context.take_last_error(), Some(1404));
}
