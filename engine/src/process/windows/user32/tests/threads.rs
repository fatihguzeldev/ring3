use super::*;
use crate::windows::user32::Message32;

#[path = "threads/queue.rs"]
mod queue;

fn finish(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    mut outcome: User32Outcome32,
) -> u32 {
    loop {
        match outcome {
            User32Outcome32::Return(value) => return value,
            User32Outcome32::Callback(callback) => {
                outcome = context
                    .complete(
                        callback.continuation,
                        u32::from(callback.arguments[1] == 0x81),
                        memory,
                    )
                    .unwrap();
            }
            User32Outcome32::Wait { .. } => panic!("window creation cannot wait"),
        }
    }
}
#[test]
fn parked_thread_continuations_retain_disjoint_scratch_and_do_not_steal_other_thread_stack() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let args = [
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
        0,
    ];
    let first = context
        .prepare_arguments(User32Api32::CreateWindowExA, &args, &mut memory, call)
        .unwrap();
    let User32Outcome32::Callback(first_callback) = first else {
        panic!("first thread must call wndproc");
    };
    let first_address = first_callback.arguments[3];
    let mut first_bytes = [0; 80];
    memory
        .read(GuestAddress(first_address), &mut first_bytes)
        .unwrap();
    let child_empty = context.empty_thread_reply();
    let first_reply = context.take_thread_reply();
    assert_eq!(
        first_reply.scratch_ranges().collect::<Vec<_>>(),
        vec![PageRange::new(GuestAddress(first_address), 1).unwrap()]
    );
    context.restore_thread_reply(child_empty);
    let child_call = User32CallContext32 {
        thread_id: call.thread_id + 1,
        ..call
    };
    let second = context
        .prepare_arguments(User32Api32::CreateWindowExA, &args, &mut memory, child_call)
        .unwrap();
    let User32Outcome32::Callback(second_callback) = second else {
        panic!("child must call wndproc");
    };
    assert_ne!(first_address, second_callback.arguments[3]);
    let mut unchanged = [0; 80];
    memory
        .read(GuestAddress(first_address), &mut unchanged)
        .unwrap();
    assert_eq!(unchanged, first_bytes);
    assert_eq!(
        context.complete(first_callback.continuation, 1, &mut memory),
        Err(User32Error32::InvalidToken)
    );
    let child = finish(&mut context, &mut memory, second);
    let child_reply = context.take_thread_reply();
    context.restore_thread_reply(first_reply);
    let first = finish(&mut context, &mut memory, first);
    assert_ne!(first, child);
    assert!(memory.permissions_at(GuestAddress(first_address)).is_err());
    assert_eq!(
        context
            .windows()
            .get(WindowHandle32::try_from(child).unwrap())
            .unwrap()
            .description
            .thread_id,
        child_call.thread_id
    );
    let parent = context.take_thread_reply();
    context.restore_thread_reply(child_reply);
    assert!(context.pending.is_empty());
    context.take_thread_reply();
    context.restore_thread_reply(parent);
}
#[test]
fn each_thread_observes_only_its_queue_and_remote_result_is_delivered_to_its_exact_parked_wait() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let child = User32CallContext32 {
        thread_id: 8,
        ..call
    };
    let hwnd = create(&mut context, &mut memory, child, atom);
    context
        .state
        .post(Message32 {
            thread_id: call.thread_id,
            message: 0x400,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(context.wait_queue_status(call.thread_id).0, 0);
    assert_eq!(context.wait_queue_status(child.thread_id).0, 0);
    context.state.observe_queue(call.thread_id, u32::MAX);
    assert_eq!(context.wait_queue_status(call.thread_id).1, 0);
    context
        .state
        .post(Message32 {
            thread_id: child.thread_id,
            message: 0x401,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(context.wait_queue_status(child.thread_id).1, 0);
    assert_eq!(context.wait_queue_status(call.thread_id).1, 0);
    let outcome = context
        .prepare_arguments(
            User32Api32::SendMessageW,
            &[hwnd, 0x400, 3, 4],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Wait { continuation } = outcome else {
        panic!("cross-thread send must park, never run WndProc in caller");
    };
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        outcome
    );
    let mut parked = context.take_thread_reply();
    assert_eq!(
        parked.accept_remote_result(continuation + 1, 42),
        Err(User32Error32::InvalidToken)
    );
    parked.accept_remote_result(continuation, 42).unwrap();
    assert_eq!(
        parked.accept_remote_result(continuation, 99),
        Err(User32Error32::InvalidToken)
    );
    context.restore_thread_reply(parked);
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Return(42)
    );
    assert!(context.pending.is_empty());
}

#[test]
fn background_thread_layout_policy_does_not_retranslate_foreground_backlog() {
    use crate::input::{
        InputKind,
        keyboard::KeyboardLayout32,
        source::{SourceEvent32, SourceKind32},
    };
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let second_call = User32CallContext32 {
        thread_id: 8,
        ..call
    };
    let second = create(&mut context, &mut memory, second_call, atom);
    let deliver = |context: &mut User32Context32, kind| {
        context
            .deliver_input(
                SourceEvent32 {
                    kind,
                    ..Default::default()
                },
                0,
            )
            .unwrap()
    };
    deliver(
        &mut context,
        SourceKind32::Window {
            focus: first,
            capture: 0,
        },
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Focus { focused: true }),
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Key {
            code: 0x0c,
            pressed: true,
        }),
    );
    deliver(
        &mut context,
        SourceKind32::ThreadTranslationPolicy {
            thread: 8,
            layout: KeyboardLayout32::TurkishQ,
            swapped: false,
        },
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Key {
            code: 0x0c,
            pressed: false,
        }),
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Key {
            code: 0x0c,
            pressed: true,
        }),
    );
    deliver(
        &mut context,
        SourceKind32::Window {
            focus: second,
            capture: 0,
        },
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Key {
            code: 0x0c,
            pressed: false,
        }),
    );
    deliver(
        &mut context,
        SourceKind32::Physical(InputKind::Key {
            code: 0x0c,
            pressed: true,
        }),
    );
    for _ in 0..3 {
        let (index, message) = context
            .state
            .find_message(call.thread_id, 0, 0x100, 0x101)
            .unwrap()
            .unwrap();
        assert_eq!(message.wparam, 0xbd);
        context.state.remove_message(index);
    }
    for _ in 0..2 {
        let (index, message) = context
            .state
            .find_message(8, 0, 0x100, 0x101)
            .unwrap()
            .unwrap();
        assert_eq!(message.wparam, 0xdf);
        context.state.remove_message(index);
    }
}

#[test]
fn sent_message_deferral_preserves_queued_msg_output_until_target_callback_finishes() {
    use crate::{
        cpu::x86::State32,
        windows::{CallFrame32, CallingConvention32},
    };
    let (mut context, mut memory, call) = fixture();
    context
        .state
        .post(Message32 {
            thread_id: call.thread_id,
            message: 0x4321,
            wparam: 0x12345678,
            ..Default::default()
        })
        .unwrap();
    memory.write(GuestAddress(0x10200), &[0xa5; 28]).unwrap();
    let args: [u32; 5] = [0x10200, 0, 0, 0, 1];
    for (index, value) in std::iter::once(0x70000).chain(args).enumerate() {
        memory
            .write(
                GuestAddress(0x11e00 + index as u32 * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let frame = CallFrame32::capture(
        &memory,
        State32 {
            registers: [0, 0, 0, 0, 0x11e00, 0, 0, 0],
            eip: 0x70000,
            eflags: 2,
        },
        CallingConvention32::Stdcall,
        5,
    )
    .unwrap();
    let User32Outcome32::Wait { continuation } = context
        .defer_retrieval(User32Api32::PeekMessageW, &frame, call)
        .unwrap()
    else {
        panic!("incoming sent callback precedes queued retrieval");
    };
    let mut bytes = [0; 28];
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 28]);
    assert!(context.waiting_retrieval(continuation));
    assert!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0x4321);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        0x12345678
    );
    assert!(
        context
            .state
            .find_message(call.thread_id, 0, 0, 0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn process_layout_policy_updates_parked_threads_without_reinterpreting_older_delivery() {
    use crate::input::{keyboard::KeyboardLayout32, source::InputCursor32};
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    context
        .state
        .windows_mut()
        .set_focus(
            call.thread_id,
            Some(WindowHandle32::try_from(window).unwrap()),
        )
        .unwrap();
    context.input.cursor = Some(InputCursor32 {
        epoch: 1,
        sequence: 0,
    });
    let parked = context.take_thread_reply();
    let empty = context.empty_thread_reply();
    context.restore_thread_reply(empty);
    let child = User32CallContext32 {
        thread_id: 8,
        ..call
    };
    memory.write(GuestAddress(0x10180), b"0000041F\0").unwrap();
    assert!(matches!(
        context
            .prepare_arguments(
                User32Api32::LoadKeyboardLayoutA,
                &[0x10180, 0],
                &mut memory,
                child
            )
            .unwrap(),
        User32Outcome32::Return(_)
    ));
    assert_eq!(
        context.resolve_thread_keyboard_layout(&parked),
        KeyboardLayout32::Us.handle()
    );
    assert_eq!(
        context
            .prepare_arguments(
                User32Api32::ActivateKeyboardLayout,
                &[KeyboardLayout32::TurkishQ.handle(), 0x100],
                &mut memory,
                child
            )
            .unwrap(),
        User32Outcome32::Return(KeyboardLayout32::Us.handle())
    );
    assert_eq!(
        context.resolve_thread_keyboard_layout(&parked),
        KeyboardLayout32::TurkishQ.handle()
    );
    assert_eq!(
        context.queried_thread_layout(call.thread_id),
        KeyboardLayout32::TurkishQ.handle()
    );
    assert_eq!(
        context.delivered_thread_layout(call.thread_id),
        KeyboardLayout32::Us
    );
    let _child = context.take_thread_reply();
    context.restore_thread_reply(parked);
    assert_eq!(context.input.layout, KeyboardLayout32::TurkishQ);
    assert!(context.input.loaded[1]);
    let outcome = context
        .prepare_arguments(
            User32Api32::ActivateKeyboardLayout,
            &[KeyboardLayout32::Us.handle(), 0x100],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Callback(callback) = outcome else {
        panic!("process activation notifies caller-owned focus");
    };
    assert_eq!(
        callback.arguments,
        [window, 0x51, 0, KeyboardLayout32::Us.handle()]
    );
    assert_eq!(
        context
            .complete(callback.continuation, 0xdeadbeef, &mut memory)
            .unwrap(),
        User32Outcome32::Return(KeyboardLayout32::TurkishQ.handle())
    );
}

#[test]
fn thread_retirement_removes_only_owner_windows_and_queue_without_guest_callbacks() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let first = create(&mut context, &mut memory, call, atom);
    let child = User32CallContext32 {
        thread_id: 8,
        ..call
    };
    let second = create(&mut context, &mut memory, child, atom);
    for (thread, window) in [(call.thread_id, first), (child.thread_id, second)] {
        context
            .state
            .post(Message32 {
                window,
                thread_id: thread,
                message: 0x400,
                ..Default::default()
            })
            .unwrap();
    }
    context
        .state
        .windows_mut()
        .set_capture(
            child.thread_id,
            Some(WindowHandle32::try_from(second).unwrap()),
        )
        .unwrap();
    let before = memory.transaction_version();
    let prepared = context.prepare_thread_termination(call.thread_id).unwrap();
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(first).unwrap())
            .is_ok()
    );
    context.commit_thread_termination(prepared);
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(first).unwrap())
            .is_err()
    );
    assert!(
        context
            .windows()
            .get(WindowHandle32::try_from(second).unwrap())
            .is_ok()
    );
    assert_eq!(context.windows().capture().unwrap().get(), second);
    assert_eq!(context.wait_queue_status(call.thread_id).0, 0);
    assert_ne!(context.wait_queue_status(child.thread_id).0, 0);
    assert_eq!(context.queried_thread_layout(call.thread_id), 0);
    assert_eq!(memory.transaction_version(), before);
    assert!(context.pending.is_empty());
}
