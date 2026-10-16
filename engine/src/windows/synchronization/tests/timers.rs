use super::*;
fn timer(context: &mut SynchronizationContext32, manual: bool) -> u32 {
    let (prepared, existed) = context.prepare_timer(manual, false, None).unwrap();
    assert!(!existed);
    context.commit_object_handle(prepared)
}
#[test]
fn timer_wait_consumption_cancel_and_set_preserve_distinct_signal_rules() {
    let mut context = context();
    let auto = timer(&mut context, false);
    let manual = timer(&mut context, true);
    for handle in [auto, manual] {
        let plan = context.prepare_set_timer(handle, 100, 0, 0, 0).unwrap();
        context.commit_signals(plan);
    }
    assert_eq!(context.next_timer_deadline(), Some(100));
    context.observe_time(99).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[auto], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    context.observe_time(100).unwrap();
    for handle in [auto, manual] {
        let plan = context.prepare_cancel_timer(handle).unwrap();
        context.commit_signals(plan);
    }
    assert_eq!(
        ready(
            context
                .wait(&[auto], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        ready(
            context
                .wait(&[auto], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    for _ in 0..2 {
        assert_eq!(
            ready(
                context
                    .wait(&[manual], WaitMode32::Any, 0, false, None)
                    .unwrap()
            ),
            0
        );
    }
    let plan = context.prepare_set_timer(manual, 200, 0, 0, 0).unwrap();
    context.commit_signals(plan);
    assert_eq!(
        ready(
            context
                .wait(&[manual], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        context.prepare_event_bindings(&[auto], &[]).err(),
        Some(SynchronizationError32::InvalidHandle)
    );
}
#[test]
fn periodic_timer_coalesces_queued_apc_and_alertable_wait_preserves_auto_signal() {
    let mut context = context();
    let handle = timer(&mut context, false);
    let plan = context
        .prepare_set_timer(handle, 10, 10, 0x1000, 77)
        .unwrap();
    context.commit_signals(plan);
    let wait = pending(
        context
            .wait(&[handle], WaitMode32::Any, INFINITE, true, None)
            .unwrap(),
    );
    let plan = context.prepare_time(1000).unwrap();
    assert_eq!(plan.timer_apcs().len(), 1);
    let apc = plan.timer_apcs()[0];
    assert_eq!(apc.argument, 77);
    assert_eq!(apc.due_ns, 10);
    context.commit_signals(plan);
    assert_eq!(context.wait_status(wait).unwrap().unwrap().value, 0xc0);
    assert_eq!(context.next_timer_deadline(), Some(1010));
    let plan = context.prepare_time(2000).unwrap();
    assert!(plan.timer_apcs().is_empty());
    context.commit_signals(plan);
    let delivery = context
        .prepare_timer_apc_delivery(apc.object, apc.generation, 1, false)
        .unwrap();
    context.commit_signals(delivery);
    assert_eq!(
        ready(
            context
                .wait(&[handle], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    let plan = context.prepare_time(2010).unwrap();
    assert_eq!(plan.timer_apcs().len(), 1);
}
#[test]
fn canceling_last_timer_apc_rearms_alertable_wait_and_thread_exit_only_cancels_callback_timer() {
    let mut context = context();
    let callback = timer(&mut context, false);
    let plain = timer(&mut context, false);
    let plan = context
        .prepare_set_timer(callback, 10, 10, 0x1000, 0)
        .unwrap();
    context.commit_signals(plan);
    let plan = context.prepare_set_timer(plain, 30, 0, 0, 0).unwrap();
    context.commit_signals(plan);
    let wait = pending(
        context
            .wait(&[], WaitMode32::Sleep, INFINITE, true, None)
            .unwrap(),
    );
    context.observe_time(10).unwrap();
    assert_eq!(context.wait_status(wait).unwrap().unwrap().value, 0xc0);
    let mut plan = context.prepare_cancel_timer(callback).unwrap();
    context
        .project_cancel_apc_state(&mut plan, 1, false)
        .unwrap();
    context.commit_signals(plan);
    assert!(context.wait_status(wait).unwrap().is_none());
    let plan = context
        .prepare_set_timer(callback, 20, 10, 0x1000, 0)
        .unwrap();
    context.commit_signals(plan);
    let plan = context.prepare_thread_exit(1).unwrap();
    context.commit_signals(plan);
    assert_eq!(context.next_timer_deadline(), Some(30));
    context.observe_time(30).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[plain], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
}
