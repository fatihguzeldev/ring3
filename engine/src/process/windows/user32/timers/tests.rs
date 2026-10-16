use super::super::{User32Outcome32, tests::fixture};
use crate::{memory::GuestAddress, windows::user32::User32Api32 as Api};

#[test]
fn minimum_interval_replacement_lazy_priority_and_killed_posted_message() {
    let (mut context, mut memory, call) = fixture();
    let User32Outcome32::Return(id) = context
        .prepare_arguments(Api::SetTimer, &[0, 999, 1, 0], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(id, 0);
    assert_ne!(id, 999);
    assert_eq!(context.timer_deadline(), Some(10_000_000));
    context.timers.observe_clock(10_000_000, 10);
    context
        .prepare_arguments(Api::PostMessageA, &[0, 0x400, 5, 6], &mut memory, call)
        .unwrap();
    context
        .prepare_arguments(Api::PeekMessageA, &[0x10200, 0, 0, 0, 1], &mut memory, call)
        .unwrap();
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x400, 5, 6]
    );
    context
        .prepare_arguments(Api::PeekMessageA, &[0x10200, 0, 0, 0, 0], &mut memory, call)
        .unwrap();
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x113, id, 0]
    );
    assert_eq!(
        context
            .prepare_arguments(Api::KillTimer, &[0, id], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        context
            .prepare_arguments(Api::PeekMessageA, &[0x10200, 0, 0, 0, 1], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x113, id, 0]
    );
    assert_eq!(
        context
            .prepare_arguments(Api::PeekMessageA, &[0x10200, 0, 0, 0, 1], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(
        context
            .prepare_arguments(Api::KillTimer, &[0, id], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(0)
    );
}

#[test]
fn timer_output_fault_preserves_due_and_callback_uses_dispatch_tick_not_message_tick() {
    let (mut context, mut memory, mut call) = fixture();
    let User32Outcome32::Return(id) = context
        .prepare_arguments(Api::SetTimer, &[0, 0, 10, 0x70000], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    context.timers.observe_clock(10_000_000, 10);
    assert!(
        context
            .prepare_arguments(Api::PeekMessageA, &[0x30000, 0, 0, 0, 1], &mut memory, call)
            .is_err()
    );
    assert!(context.timer_message_ready(call.thread_id, 0, 0, 0));
    assert_eq!(
        context
            .prepare_arguments(Api::PeekMessageA, &[0x10200, 0, 0, 0, 1], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let mut bytes = [0; 28];
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 10);
    call.time_ms = 15;
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(Api::DispatchMessageA, &[0x10200], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(callback.arguments, [0, 0x113, id, 15]);
    assert_eq!(
        context
            .complete(callback.continuation, 0xfeed, &mut memory)
            .unwrap(),
        User32Outcome32::Return(0)
    );
}

#[test]
fn blocking_timer_wait_is_thread_local_and_replacement_restarts_deadline() {
    let (mut context, mut memory, call) = fixture();
    let User32Outcome32::Return(id) = context
        .prepare_arguments(Api::SetTimer, &[0, 0, 10, 0], &mut memory, call)
        .unwrap()
    else {
        panic!()
    };
    let User32Outcome32::Wait { continuation } = context
        .prepare_arguments(
            Api::GetMessageA,
            &[0x10200, 0, 0x113, 0x113],
            &mut memory,
            call,
        )
        .unwrap()
    else {
        panic!()
    };
    context.timers.observe_clock(5_000_000, 5);
    assert_eq!(
        context
            .prepare_arguments(Api::SetTimer, &[0, id, 20, 0], &mut memory, call)
            .unwrap(),
        User32Outcome32::Return(id)
    );
    context.timers.observe_clock(10_000_000, 10);
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Wait { continuation }
    );
    context.timers.observe_clock(25_000_000, 25);
    assert!(!context.timer_message_ready(call.thread_id + 1, 0, 0, 0));
    assert_eq!(
        context.resume_wait(continuation, &mut memory).unwrap(),
        User32Outcome32::Return(1)
    );
    assert_eq!(
        super::super::memory::words::<4>(&memory, 0x10200).unwrap(),
        [0, 0x113, id, 0]
    );
}

#[test]
fn timer_created_after_nonzero_clock_origin_uses_current_absolute_clock() {
    let (mut context, mut memory, mut call) = fixture();
    call.now_ns = 123_000_000;
    context
        .prepare_arguments(Api::SetTimer, &[0, 0, 10, 0], &mut memory, call)
        .unwrap();
    assert_eq!(context.timer_deadline(), Some(133_000_000));
    context.timers.observe_clock(132_000_000, 9);
    assert!(!context.timer_message_ready(call.thread_id, 0, 0, 0));
    context.timers.observe_clock(133_000_000, 10);
    assert!(context.timer_message_ready(call.thread_id, 0, 0, 0));
}
