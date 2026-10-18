use super::*;
use crate::windows::user32::{Message32, QueuedKey32};

const OUTPUT: u32 = 0x11000;

fn install(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    procedure: u32,
) -> u32 {
    invoke(
        context,
        memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[2, procedure, 0, call.thread_id],
    )
}

fn key(context: &mut User32Context32, call: User32CallContext32, virtual_key: u8, pressed: bool) {
    let mut state = [0; 256];
    state[usize::from(virtual_key)] = if pressed { 0x80 } else { 0 };
    context
        .state
        .post_input(
            Message32 {
                message: if pressed { 0x100 } else { 0x101 },
                wparam: u32::from(virtual_key),
                lparam: if pressed { 0x0140_0001 } else { 0xc140_0001 },
                thread_id: call.thread_id,
                ..Default::default()
            },
            Some(QueuedKey32 {
                virtual_key,
                pressed,
                repeated: false,
            }),
            state,
        )
        .unwrap();
}

fn peek(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    flags: u32,
) -> User32Outcome32 {
    context
        .prepare_arguments(
            User32Api32::PeekMessageA,
            &[OUTPUT, 0, 0x100, 0x109, flags],
            memory,
            call,
        )
        .unwrap()
}

fn callback(outcome: User32Outcome32) -> User32Callback32 {
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("keyboard callback required, got {outcome:?}")
    };
    assert_eq!(callback.count, 3);
    callback
}

#[test]
fn physical_keyboard_callbacks_inherit_activation_but_explicit_none_remains_captured() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    key(&mut context, call, 0x28, true);
    let hook = callback(peek(&mut context, &mut memory, call, 0));
    assert_eq!(context.callback_activation(hook.continuation), None);
    context.pending.last_mut().unwrap().activation =
        Some(crate::windows::activation::CapturedActivation32::new(0));
    assert_eq!(context.callback_activation(hook.continuation), Some(0));
}

#[test]
fn keyboard_noremove_repeats_and_remove_updates_key_state_before_the_hook() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    key(&mut context, call, 0x28, true);
    for _ in 0..2 {
        let hook = callback(peek(&mut context, &mut memory, call, 0));
        assert_eq!(hook.arguments[..3], [3, 0x28, 0x0140_0001]);
        assert_eq!(context.input.queued[0x28], 0);
        assert_eq!(
            context.complete(hook.continuation, 0, &mut memory).unwrap(),
            User32Outcome32::Return(1)
        );
        assert!(
            context
                .state
                .find_message(call.thread_id, 0, 0, 0)
                .unwrap()
                .is_some()
        );
    }
    let hook = callback(peek(&mut context, &mut memory, call, 1));
    assert_eq!(hook.arguments[..3], [0, 0x28, 0x0140_0001]);
    assert_eq!(context.input.queued[0x28], 0x80);
    assert!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        context.complete(hook.continuation, 0, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::super::memory::message(&memory, OUTPUT)
            .unwrap()
            .wparam,
        0x28
    );
}

#[test]
fn keyboard_chain_is_typed_and_survives_self_unhook() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    let newest = install(&mut context, &mut memory, call, 0x70200);
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::SetWindowsHookExA,
        &[5, 0x70300, 0, call.thread_id],
    );
    key(&mut context, call, 0x41, true);
    let outer = callback(peek(&mut context, &mut memory, call, 1));
    assert_eq!(outer.procedure.0, 0x70200);
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::UnhookWindowsHookEx,
        &[newest],
    );
    let inner = callback(
        context
            .prepare_arguments(
                User32Api32::CallNextHookEx,
                &[0, 0, 0x41, 0x0140_0001],
                &mut memory,
                call,
            )
            .unwrap(),
    );
    assert_eq!(inner.procedure.0, 0x70100);
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::CallNextHookEx,
            &[0, 0, 0x41, 0x0140_0001]
        ),
        0
    );
    assert_eq!(
        context
            .complete(inner.continuation, 77, &mut memory)
            .unwrap(),
        User32Outcome32::Return(77)
    );
    assert_eq!(
        context
            .complete(outer.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(1)
    );
}

#[test]
fn suppressing_noremove_consumes_the_key_and_retrieves_the_next_message() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    memory.write(GuestAddress(OUTPUT), &[0xaa; 28]).unwrap();
    key(&mut context, call, 0x41, true);
    key(&mut context, call, 0x42, true);
    let first = callback(peek(&mut context, &mut memory, call, 0));
    let second = callback(
        context
            .complete(first.continuation, 1, &mut memory)
            .unwrap(),
    );
    assert_eq!(second.arguments[..2], [3, 0x42]);
    let mut bytes = [0; 28];
    memory.read(GuestAddress(OUTPUT), &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 28]);
    assert_eq!(
        context
            .complete(second.continuation, 0, &mut memory)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::super::memory::message(&memory, OUTPUT)
            .unwrap()
            .wparam,
        0x42
    );
}

#[test]
fn reentrant_removal_cannot_make_outer_suppression_remove_a_replacement_key() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    key(&mut context, call, 0x41, true);
    let outer = callback(peek(&mut context, &mut memory, call, 0));
    let nested = callback(peek(&mut context, &mut memory, call, 1));
    context
        .complete(nested.continuation, 0, &mut memory)
        .unwrap();
    key(&mut context, call, 0x41, true);
    let replacement = callback(
        context
            .complete(outer.continuation, 1, &mut memory)
            .unwrap(),
    );
    assert_eq!(replacement.arguments[..2], [3, 0x41]);
    context
        .complete(replacement.continuation, 0, &mut memory)
        .unwrap();
    assert!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .is_some()
    );
}

#[test]
fn keyboard_hooks_exclude_posts_and_filtered_input_and_preflight_output() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    invoke(
        &mut context,
        &mut memory,
        call,
        User32Api32::PostMessageA,
        &[0, 0x100, 0x41, 0],
    );
    assert_eq!(
        peek(&mut context, &mut memory, call, 1),
        User32Outcome32::Return(1)
    );
    key(&mut context, call, 0x42, true);
    assert_eq!(
        invoke(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageA,
            &[OUTPUT, 0, 0x400, 0x400, 1]
        ),
        0
    );
    let before = context.pending.len();
    assert!(
        context
            .prepare_arguments(
                User32Api32::PeekMessageA,
                &[0x90000, 0, 0, 0, 1],
                &mut memory,
                call
            )
            .is_err()
    );
    assert_eq!(context.pending.len(), before);
    assert!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .is_some()
    );
    assert_eq!(context.input.queued[0x42], 0);
}

#[test]
fn a_blocked_getmessage_resumes_through_the_same_keyboard_hook_path() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    let User32Outcome32::Wait { continuation } = context
        .prepare_arguments(
            User32Api32::GetMessageA,
            &[OUTPUT, 0, 0x100, 0x109],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!("empty GetMessage must wait")
    };
    key(&mut context, call, 0x41, true);
    let hook = callback(context.resume_wait(continuation, &mut memory).unwrap());
    assert_eq!(hook.arguments[..3], [0, 0x41, 0x0140_0001]);
    assert_eq!(context.input.queued[0x41], 0x80);
    assert_eq!(
        context.complete(hook.continuation, 0, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::super::memory::message(&memory, OUTPUT)
            .unwrap()
            .wparam,
        0x41
    );
}

#[test]
fn failed_output_completion_retries_the_saved_hook_decision_without_another_dequeue() {
    let (mut context, mut memory, call, _) = setup();
    install(&mut context, &mut memory, call, 0x70100);
    key(&mut context, call, 0x41, true);
    let hook = callback(peek(&mut context, &mut memory, call, 1));
    key(&mut context, call, 0x42, true);
    let output = PageRange::new(GuestAddress(OUTPUT), 1).unwrap();
    memory.protect(output, Permissions::READ).unwrap();
    assert!(context.complete(hook.continuation, 0, &mut memory).is_err());
    assert_eq!(context.pending.last().unwrap().token, hook.continuation);
    memory.protect(output, Permissions::READ_WRITE).unwrap();
    assert_eq!(
        context.complete(hook.continuation, 1, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::super::memory::message(&memory, OUTPUT)
            .unwrap()
            .wparam,
        0x41
    );
    assert_eq!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .unwrap()
            .1
            .wparam,
        0x42
    );
}

#[test]
fn keyboard_removal_preserves_raw_packet_retirement_but_noremove_does_not() {
    use crate::{input::InputKind, windows::rawinput::RawRegistration32};
    for (remove, suppress) in [(false, false), (true, false), (false, true)] {
        let (mut context, mut memory, call, _) = setup();
        install(&mut context, &mut memory, call, 0x70100);
        let rows = context
            .raw_input
            .prepare_registrations(&[RawRegistration32 {
                page: 1,
                usage: 6,
                flags: 0x100,
                window: 7,
            }])
            .unwrap();
        context.raw_input.commit_registrations(rows);
        let packet = context
            .raw_input
            .prepare_packet(
                InputKind::Key {
                    code: 30,
                    pressed: true,
                },
                65,
                false,
                0,
                true,
                |window| (window == 7).then_some(call.thread_id),
            )
            .unwrap()
            .unwrap();
        let id = packet.packet().id;
        context.raw_input.commit_packet(packet);
        context.raw_input.mark_delivered(call.thread_id, id);
        assert!(context.raw_input.packet(id, call.thread_id).is_ok());
        key(&mut context, call, 0x41, true);
        let hook = callback(peek(&mut context, &mut memory, call, u32::from(remove)));
        assert_eq!(
            context.raw_input.packet(id, call.thread_id).is_err(),
            remove
        );
        let outcome = context
            .complete(hook.continuation, u32::from(suppress), &mut memory)
            .unwrap();
        assert_eq!(outcome, User32Outcome32::Return(u32::from(!suppress)));
        assert_eq!(
            context.raw_input.packet(id, call.thread_id).is_err(),
            remove || suppress
        );
    }
}
