use super::*;

fn query(
    context: &mut User32Context32,
    memory: &mut AddressSpace,
    call: User32CallContext32,
    api: User32Api32,
    args: &[u32],
) -> u32 {
    let User32Outcome32::Return(value) =
        context.prepare_arguments(api, args, memory, call).unwrap()
    else {
        panic!("queue query must complete");
    };
    value
}

#[test]
fn sent_queue_queries_and_peek_observe_only_the_calling_thread_without_consuming_invocations() {
    let (mut context, mut memory, call) = fixture();
    let child = User32CallContext32 {
        thread_id: call.thread_id + 1,
        ..call
    };
    let arrival = context.prepare_sent_change(call.thread_id, 1).unwrap();
    context.commit_sent_change(arrival);
    let arrival = context.prepare_sent_change(child.thread_id, 1).unwrap();
    context.commit_sent_change(arrival);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetInputState,
            &[]
        ),
        0
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            child,
            User32Api32::GetQueueStatus,
            &[0x40]
        ),
        0x00400040
    );
    assert_eq!(context.wait_queue_status(call.thread_id), (0x40, 0x40, 2));
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetQueueStatus,
            &[0x40]
        ),
        0x00400040
    );
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::GetQueueStatus,
            &[0x40]
        ),
        0x00400000
    );
    let arrival = context.prepare_sent_change(call.thread_id, 1).unwrap();
    context.commit_sent_change(arrival);
    assert_eq!(
        query(
            &mut context,
            &mut memory,
            call,
            User32Api32::PeekMessageW,
            &[0x10000, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(context.wait_queue_status(call.thread_id), (0x40, 0, 3));
    let removal = context.prepare_sent_change(call.thread_id, -1).unwrap();
    context.commit_sent_change(removal);
    assert_eq!(context.wait_queue_status(call.thread_id), (0x40, 0, 4));
    let removal = context.prepare_sent_change(call.thread_id, -1).unwrap();
    context.commit_sent_change(removal);
    assert_eq!(context.wait_queue_status(call.thread_id), (0, 0, 5));
    assert_eq!(context.wait_queue_status(child.thread_id), (0x40, 0, 5));
}

#[test]
fn message_wait_observation_uses_the_child_queue_and_thread_exit_reclaims_sent_state() {
    let (mut context, _, call) = fixture();
    let child = call.thread_id + 1;
    let arrival = context.prepare_sent_change(call.thread_id, 1).unwrap();
    context.commit_sent_change(arrival);
    let arrival = context.prepare_sent_change(child, 1).unwrap();
    context.commit_sent_change(arrival);
    context.observe_wait_queue(child, 0x40);
    assert_eq!(context.wait_queue_status(child), (0x40, 0, 2));
    assert_eq!(context.wait_queue_status(call.thread_id), (0x40, 0x40, 2));
    let exit = context.prepare_thread_termination(child).unwrap();
    context.commit_thread_termination(exit);
    assert_eq!(context.wait_queue_status(child), (0, 0, 2));
    assert_eq!(context.wait_queue_status(call.thread_id), (0x40, 0x40, 2));
}
