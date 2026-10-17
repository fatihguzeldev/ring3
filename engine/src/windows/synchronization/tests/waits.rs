use super::*;

#[test]
fn wait_any_chooses_lowest_index_and_wait_all_consumes_nothing_until_every_object_is_ready() {
    let mut context = context();
    let (a, _) = context.create_event(false, true, false, None).unwrap();
    let (b, _) = context.create_event(false, false, false, None).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[a, b], WaitMode32::All, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert!(context.event_is_signaled(a).unwrap());
    context.set_event(b).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[b, a], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert!(context.event_is_signaled(a).unwrap());
    assert!(!context.event_is_signaled(b).unwrap());
    context.set_event(b).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[a, b], WaitMode32::All, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert!(!context.event_is_signaled(a).unwrap());
    assert!(!context.event_is_signaled(b).unwrap());
    let alias = context
        .prepare_duplicate(a, 0, false, true, false, true)
        .unwrap();
    let duplicate = alias.handle().unwrap();
    context.commit_duplicate(alias);
    context.set_event(a).unwrap();
    assert_eq!(
        context.wait(&[a, duplicate], WaitMode32::All, 0, false, None),
        Err(SynchronizationError32::InvalidWait)
    );
    assert!(context.event_is_signaled(a).unwrap());
    assert_eq!(
        ready(
            context
                .wait(&[duplicate, a], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
}

#[test]
fn auto_signal_releases_one_current_waiter_and_manual_pulse_releases_all_without_latching() {
    let mut context = context();
    let (auto, _) = context.create_event(false, false, false, None).unwrap();
    let first = pending(
        context
            .wait(&[auto], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    let second = pending(
        context
            .wait(&[auto], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    context.set_event(auto).unwrap();
    context.reset_event(auto).unwrap();
    assert_eq!(context.wait_status(first).unwrap().unwrap().value, 0);
    assert_eq!(context.wait_status(second).unwrap(), None);
    context.pulse_event(auto).unwrap();
    assert_eq!(context.wait_status(second).unwrap().unwrap().value, 0);
    assert!(!context.event_is_signaled(auto).unwrap());
    let (manual, _) = context.create_event(true, false, false, None).unwrap();
    let a = pending(
        context
            .wait(&[manual], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    let b = pending(
        context
            .wait(&[manual], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    context.pulse_event(manual).unwrap();
    assert_eq!(context.wait_status(a).unwrap().unwrap().value, 0);
    assert_eq!(context.wait_status(b).unwrap().unwrap().value, 0);
    assert!(!context.event_is_signaled(manual).unwrap());
    let binding = context.retain_event(auto).unwrap();
    let a = pending(
        context
            .wait(&[auto], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    let b = pending(
        context
            .wait(&[auto], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    let prepared = context.prepare_signals(&[binding, binding]).unwrap();
    context.commit_signals(prepared);
    assert_eq!(context.wait_status(a).unwrap().unwrap().value, 0);
    assert_eq!(context.wait_status(b).unwrap().unwrap().value, 0);
    assert!(!context.event_is_signaled(auto).unwrap());
    let prepared = context
        .prepare_signal_occurrences(&[(binding, u64::MAX)])
        .unwrap();
    context.commit_signals(prepared);
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
}

#[test]
fn pulse_cannot_partially_satisfy_wait_all_or_create_a_signal_for_a_future_waiter() {
    let mut context = context();
    let (a, _) = context.create_event(false, false, false, None).unwrap();
    let (b, _) = context.create_event(false, false, false, None).unwrap();
    let all = pending(
        context
            .wait(&[a, b], WaitMode32::All, INFINITE, false, None)
            .unwrap(),
    );
    context.pulse_event(a).unwrap();
    context.set_event(b).unwrap();
    assert_eq!(context.wait_status(all).unwrap(), None);
    assert!(context.event_is_signaled(b).unwrap());
    context.set_event(a).unwrap();
    assert_eq!(context.wait_status(all).unwrap().unwrap().value, 0);
    assert_eq!(
        ready(
            context
                .wait(&[a, b], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
}

#[test]
fn finite_deadline_and_infinite_sleep_use_observed_clock_and_late_signal_is_not_consumed() {
    let mut context = context();
    let (event, _) = context.create_event(false, false, false, None).unwrap();
    let finite = pending(
        context
            .wait(&[event], WaitMode32::Any, 10, false, None)
            .unwrap(),
    );
    let infinite = pending(
        context
            .wait(&[], WaitMode32::Sleep, INFINITE, true, None)
            .unwrap(),
    );
    context.observe_time(9_999_999).unwrap();
    assert_eq!(context.wait_status(finite).unwrap(), None);
    context.observe_time(10_000_000).unwrap();
    context.set_event(event).unwrap();
    assert_eq!(
        context.take_wait(finite).unwrap().unwrap().value,
        WAIT_TIMEOUT
    );
    assert!(context.event_is_signaled(event).unwrap());
    assert_eq!(context.wait_status(infinite).unwrap(), None);
    let revision = context.revision();
    assert_eq!(
        context.observe_time(9),
        Err(SynchronizationError32::ClockBackwards)
    );
    assert_eq!(context.revision(), revision);
    context.cancel_wait(infinite).unwrap();
    assert_eq!(
        context.wait_status(infinite),
        Err(SynchronizationError32::Stale)
    );
}

#[test]
fn message_wait_distinguishes_old_input_inputavailable_and_all_objects_without_consuming_messages()
{
    let mut context = context();
    let message = MessageWait32 {
        mask: 1,
        input_available: false,
    };
    context.observe_messages(1, 0, 1).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[], WaitMode32::Any, 0, false, Some(message))
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        ready(
            context
                .wait(
                    &[],
                    WaitMode32::Any,
                    0,
                    false,
                    Some(MessageWait32 {
                        input_available: true,
                        ..message
                    })
                )
                .unwrap()
        ),
        0
    );
    let (event, _) = context.create_event(false, true, false, None).unwrap();
    let wait = pending(
        context
            .wait(&[event], WaitMode32::All, INFINITE, false, Some(message))
            .unwrap(),
    );
    assert!(context.event_is_signaled(event).unwrap());
    context.observe_messages(1, 1, 2).unwrap();
    assert_eq!(context.wait_status(wait).unwrap().unwrap().value, 0);
    assert!(!context.event_is_signaled(event).unwrap());
    assert_eq!(
        ready(
            context
                .wait(&[], WaitMode32::Any, 0, false, Some(message))
                .unwrap()
        ),
        0
    );
}

#[test]
fn close_pending_handle_fails_that_wait_and_atomic_signal_wait_validates_before_signaling() {
    let mut context = context();
    let (event, _) = context.create_event(false, false, false, None).unwrap();
    let wait = pending(
        context
            .wait(&[event], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    assert_eq!(
        context.signal_and_wait(event, 0, 0, false),
        Err(SynchronizationError32::InvalidHandle)
    );
    assert!(!context.event_is_signaled(event).unwrap());
    context.close_handle(event).unwrap();
    assert_eq!(
        context.wait_status(wait).unwrap(),
        Some(WaitResult32 {
            value: WAIT_FAILED,
            last_error: Some(6)
        })
    );
    let (new, _) = context.create_event(false, false, false, None).unwrap();
    assert_eq!(
        ready(context.signal_and_wait(new, new, 0, false).unwrap()),
        0
    );
    assert!(!context.event_is_signaled(new).unwrap());
}

#[test]
fn message_wait_observes_new_messages_once_and_inputavailable_sees_old_queue_entries() {
    let mut context = context();
    let message = MessageWait32 {
        mask: 1,
        input_available: false,
    };
    assert_eq!(
        ready(
            context
                .wait_with_messages(&[], WaitMode32::Any, 0, false, message, (1, 1, 1))
                .unwrap()
        ),
        0
    );
    assert_eq!(
        ready(
            context
                .wait(&[], WaitMode32::Any, 0, false, Some(message))
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        ready(
            context
                .wait(
                    &[],
                    WaitMode32::Any,
                    0,
                    false,
                    Some(MessageWait32 {
                        input_available: true,
                        ..message
                    })
                )
                .unwrap()
        ),
        0
    );
    context.revision = u64::MAX;
    assert_eq!(
        context.wait_with_messages(&[], WaitMode32::Any, INFINITE, false, message, (1, 1, 2)),
        Err(SynchronizationError32::Exhausted)
    );
    assert_eq!(context.queue_state(1), (1, 0, 1));
}

#[test]
fn message_waits_and_new_message_observation_are_owned_by_the_waiting_thread() {
    let mut context = context();
    let message = MessageWait32 {
        mask: 8,
        input_available: false,
    };
    context.set_current_thread(1).unwrap();
    let first = pending(
        context
            .wait(&[], WaitMode32::Any, INFINITE, false, Some(message))
            .unwrap(),
    );
    context.set_current_thread(2).unwrap();
    let second = pending(
        context
            .wait(&[], WaitMode32::Any, INFINITE, false, Some(message))
            .unwrap(),
    );
    let prepared = context
        .prepare_messages_for_threads(&[(1, 8, 8, 100), (2, 0, 0, 1)])
        .unwrap();
    context.commit_signals(prepared);
    assert!(context.wait_status(first).unwrap().is_some());
    assert_eq!(context.wait_status(second).unwrap(), None);
    context.take_wait_observing_messages(first).unwrap();
    assert_eq!(context.queue_state(1), (8, 0, 100));
    assert_eq!(context.queue_state(2), (0, 0, 1));
    context.observe_messages_for_thread(2, 8, 8, 2).unwrap();
    assert!(
        context
            .take_wait_observing_messages(second)
            .unwrap()
            .is_some()
    );
    assert_eq!(context.queue_state(2), (8, 0, 2));
}

#[test]
fn composite_message_projection_failure_preserves_every_thread_and_pending_wait() {
    let mut context = context();
    context.observe_messages_for_thread(1, 0, 0, 5).unwrap();
    context.observe_messages_for_thread(2, 0, 0, 9).unwrap();
    let revision = context.revision();
    assert!(matches!(
        context.prepare_messages_for_threads(&[(1, 8, 8, 6), (2, 8, 8, 8)]),
        Err(SynchronizationError32::Stale)
    ));
    assert_eq!(context.revision(), revision);
    assert_eq!(context.queue_state(1), (0, 0, 5));
    assert_eq!(context.queue_state(2), (0, 0, 9));
    assert!(matches!(
        context.prepare_messages_for_threads(&[(1, 8, 8, 6), (1, 8, 8, 6)]),
        Err(SynchronizationError32::InvalidWait)
    ));
    let prepared = context
        .prepare_messages_for_thread(2, u32::MAX, u32::MAX, 9)
        .unwrap();
    let prepared = context.refresh_prepared_messages_for_thread(prepared, 2, 0, 0, 10);
    context.commit_signals(prepared);
    assert_eq!(context.queue_state(2), (0, 0, 10));
}

#[test]
fn queued_apc_interrupts_only_alertable_waits_without_consuming_a_signaled_auto_event() {
    let mut context = context();
    let (event, _) = context.create_event(false, false, false, None).unwrap();
    context.set_current_thread(1).unwrap();
    let alertable = pending(
        context
            .wait(&[event], WaitMode32::Any, INFINITE, true, None)
            .unwrap(),
    );
    context.set_current_thread(2).unwrap();
    let ordinary = pending(
        context
            .wait(&[event], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    let receipt = context.prepare_apc_state(1, true).unwrap();
    context.commit_signals(receipt);
    assert_eq!(context.wait_status(alertable).unwrap().unwrap().value, 0xc0);
    assert_eq!(context.wait_status(ordinary).unwrap(), None);
    context.cancel_wait(ordinary).unwrap();
    context.take_wait(alertable).unwrap();
    context.set_event(event).unwrap();
    context.set_current_thread(1).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[event], WaitMode32::Any, 0, true, None)
                .unwrap()
        ),
        0xc0
    );
    assert!(context.event_is_signaled(event).unwrap());
    let receipt = context.prepare_apc_state(1, false).unwrap();
    context.commit_signals(receipt);
    assert_eq!(
        ready(
            context
                .wait(&[event], WaitMode32::Any, 0, true, None)
                .unwrap()
        ),
        0
    );
    assert!(!context.event_is_signaled(event).unwrap());
}

#[test]
fn signal_occurrences_and_all_thread_queue_rows_publish_in_one_prepared_revision() {
    let mut context = context();
    let (event, _) = context.create_event(true, false, false, None).unwrap();
    let binding = context.retain_event(event).unwrap();
    let initial = context
        .prepare_messages_for_threads(&[(1, 8, 8, 7), (2, 8, 8, 7)])
        .unwrap();
    context.commit_signals(initial);
    let revision = context.revision();
    assert!(matches!(
        context.prepare_signal_occurrences_with_message_rows(
            &[(binding, 1)],
            &[(1, 0, 0, 7), (2, 0, 0, 6)]
        ),
        Err(SynchronizationError32::Stale)
    ));
    assert_eq!(context.revision(), revision);
    assert!(!context.event_is_signaled(event).unwrap());
    assert_eq!(context.queue_state(1), (8, 8, 7));
    assert_eq!(context.queue_state(2), (8, 8, 7));
    let prepared = context
        .prepare_signal_occurrences_with_message_rows(
            &[(binding, 1)],
            &[(1, 0, 0, 7), (2, 0, 0, 7)],
        )
        .unwrap();
    assert!(!context.event_is_signaled(event).unwrap());
    context.commit_signals(prepared);
    assert_eq!(context.revision(), revision + 1);
    assert!(context.event_is_signaled(event).unwrap());
    assert_eq!(context.queue_state(1), (0, 0, 7));
    assert_eq!(context.queue_state(2), (0, 0, 7));
}

#[test]
fn queue_rows_resolve_waiters_even_when_all_event_occurrence_counts_are_zero() {
    let mut context = context();
    let (event, _) = context.create_event(true, false, false, None).unwrap();
    let binding = context.retain_event(event).unwrap();
    context.set_current_thread(2).unwrap();
    let wait = pending(
        context
            .wait(
                &[],
                WaitMode32::Any,
                INFINITE,
                false,
                Some(MessageWait32 {
                    mask: 8,
                    input_available: true,
                }),
            )
            .unwrap(),
    );
    let revision = context.revision();
    let prepared = context
        .prepare_signal_occurrences_with_message_rows(&[(binding, 0)], &[(2, 8, 8, 1)])
        .unwrap();
    assert_eq!(context.wait_status(wait).unwrap(), None);
    context.commit_signals(prepared);
    assert_eq!(context.revision(), revision + 1);
    assert_eq!(context.wait_status(wait).unwrap().unwrap().value, 0);
    assert!(!context.event_is_signaled(event).unwrap());
}
