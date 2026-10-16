use crate::{
    memory::GuestAddress,
    process::windows::user32::{
        User32Outcome32,
        tests::{create, fixture, register},
    },
    windows::{activation::CapturedActivation32, user32::User32Api32 as Api},
};

#[test]
fn identical_posts_keep_fifo_contexts_through_no_remove_and_guest_msg_copies() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    for handle in [0x1111, 0x2222] {
        context.call_activation = CapturedActivation32::new(handle);
        context
            .prepare_arguments(Api::PostMessageW, &[window, 0x400, 7, 8], &mut memory, call)
            .unwrap();
    }
    context.call_activation = Default::default();
    for flags in [0, 0, 1] {
        assert_eq!(
            context
                .prepare_arguments(
                    Api::PeekMessageW,
                    &[0x10200, 0, 0x400, 0x400, flags],
                    &mut memory,
                    call
                )
                .unwrap(),
            User32Outcome32::Return(1)
        );
    }
    let mut copy = [0; 28];
    memory.read(GuestAddress(0x10200), &mut copy).unwrap();
    memory.write(GuestAddress(0x10300), &copy).unwrap();
    context
        .prepare_arguments(
            Api::PeekMessageW,
            &[0x10200, 0, 0x400, 0x400, 1],
            &mut memory,
            call,
        )
        .unwrap();
    let mut second = [0; 28];
    memory.read(GuestAddress(0x10200), &mut second).unwrap();
    assert_eq!(copy, second);
    for (address, handle) in [(0x10300, 0x1111), (0x10200, 0x2222)] {
        let User32Outcome32::Callback(callback) = context
            .prepare_arguments(Api::DispatchMessageW, &[address], &mut memory, call)
            .unwrap()
        else {
            panic!("window procedure")
        };
        assert_eq!(
            context.callback_activation(callback.continuation),
            Some(handle)
        );
        context
            .complete(callback.continuation, 0, &mut memory)
            .unwrap();
    }
}

#[test]
fn undispatched_provenance_exhaustion_and_output_fault_leave_the_message_queued() {
    let (mut context, mut memory, call) = fixture();
    let atom = register(&mut context, &mut memory, call);
    let window = create(&mut context, &mut memory, call, atom);
    for value in 0..8 {
        context.call_activation = CapturedActivation32::new(0x1111);
        context
            .prepare_arguments(
                Api::PostMessageW,
                &[window, 0x400, value, 0],
                &mut memory,
                call,
            )
            .unwrap();
        context
            .prepare_arguments(
                Api::PeekMessageW,
                &[0x10200 + value * 28, 0, 0x400, 0x400, 1],
                &mut memory,
                call,
            )
            .unwrap();
    }
    context
        .prepare_arguments(
            Api::PostMessageW,
            &[window, 0x400, 99, 0],
            &mut memory,
            call,
        )
        .unwrap();
    memory.write(GuestAddress(0x10500), &[0xa5; 28]).unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        context
            .prepare_arguments(
                Api::PeekMessageW,
                &[0x10500, 0, 0x400, 0x400, 1],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(0)
    );
    assert_eq!(context.take_last_error(), Some(1816));
    assert_eq!(memory.transaction_version(), version);
    let mut output = [0; 28];
    memory.read(GuestAddress(0x10500), &mut output).unwrap();
    assert_eq!(output, [0xa5; 28]);
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(Api::DispatchMessageW, &[0x10200], &mut memory, call)
        .unwrap()
    else {
        panic!("window procedure")
    };
    context
        .complete(callback.continuation, 0, &mut memory)
        .unwrap();
    assert!(
        context
            .prepare_arguments(
                Api::PeekMessageW,
                &[0x50000, 0, 0x400, 0x400, 1],
                &mut memory,
                call
            )
            .is_err()
    );
    assert_eq!(
        context
            .prepare_arguments(
                Api::PeekMessageW,
                &[0x10500, 0, 0x400, 0x400, 1],
                &mut memory,
                call
            )
            .unwrap(),
        User32Outcome32::Return(1)
    );
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(Api::DispatchMessageW, &[0x10500], &mut memory, call)
        .unwrap()
    else {
        panic!("window procedure")
    };
    assert_eq!(callback.arguments[2], 99);
}

#[test]
fn lazy_timer_message_keeps_original_context_across_replacement_and_kill() {
    let (mut context, mut memory, mut call) = fixture();
    context.call_activation = CapturedActivation32::new(0x1111);
    let User32Outcome32::Return(id) = context
        .prepare_arguments(Api::SetTimer, &[0, 0, 10, 0x70000], &mut memory, call)
        .unwrap()
    else {
        panic!("timer")
    };
    context.timers.observe_clock(10_000_000, 10);
    context
        .prepare_arguments(
            Api::PeekMessageW,
            &[0x10200, 0, 0x113, 0x113, 0],
            &mut memory,
            call,
        )
        .unwrap();
    context.call_activation = CapturedActivation32::new(0x2222);
    call.now_ns = 10_000_000;
    context
        .prepare_arguments(Api::SetTimer, &[0, id, 10, 0x70000], &mut memory, call)
        .unwrap();
    context
        .prepare_arguments(
            Api::PeekMessageW,
            &[0x10200, 0, 0x113, 0x113, 1],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(Api::DispatchMessageW, &[0x10200], &mut memory, call)
        .unwrap()
    else {
        panic!("timer callback")
    };
    assert_eq!(
        context.callback_activation(callback.continuation),
        Some(0x1111)
    );
    context
        .complete(callback.continuation, 0, &mut memory)
        .unwrap();
    context.timers.observe_clock(20_000_000, 20);
    context
        .prepare_arguments(
            Api::PeekMessageW,
            &[0x10200, 0, 0x113, 0x113, 0],
            &mut memory,
            call,
        )
        .unwrap();
    context
        .prepare_arguments(Api::KillTimer, &[0, id], &mut memory, call)
        .unwrap();
    context
        .prepare_arguments(
            Api::PeekMessageW,
            &[0x10200, 0, 0x113, 0x113, 1],
            &mut memory,
            call,
        )
        .unwrap();
    let User32Outcome32::Callback(callback) = context
        .prepare_arguments(Api::DispatchMessageW, &[0x10200], &mut memory, call)
        .unwrap()
    else {
        panic!("timer callback")
    };
    assert_eq!(
        context.callback_activation(callback.continuation),
        Some(0x2222)
    );
}
