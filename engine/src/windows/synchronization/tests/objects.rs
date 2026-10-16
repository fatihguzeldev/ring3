use super::*;

fn context() -> SynchronizationContext32 {
    SynchronizationContext32::new(71, SynchronizationLimits32::default(), 0).unwrap()
}
fn value(outcome: WaitOutcome32) -> u32 {
    let WaitOutcome32::Ready(result) = outcome else {
        panic!()
    };
    result.value
}

#[test]
fn mixed_wait_all_consumes_mutex_semaphore_and_auto_event_only_when_every_object_is_ready() {
    let mut context = context();
    let (mutex, _) = context.create_mutex(false, false, None).unwrap();
    let (semaphore, _) = context.create_semaphore(1, 2, false, None).unwrap();
    let (event, _) = context.create_event(false, false, false, None).unwrap();
    context.set_current_thread(2).unwrap();
    assert_eq!(
        value(
            context
                .wait(&[mutex, semaphore, event], WaitMode32::All, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    context.set_event(event).unwrap();
    assert_eq!(
        value(
            context
                .wait(&[mutex, semaphore, event], WaitMode32::All, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        value(
            context
                .wait(&[semaphore], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        value(
            context
                .wait(&[event], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert_eq!(
        value(
            context
                .wait(&[mutex], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    context.set_current_thread(1).unwrap();
    assert_eq!(
        context.release_mutex(mutex),
        Err(SynchronizationError32::NotOwner)
    );
    assert_eq!(
        value(
            context
                .wait(&[mutex], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    context.set_current_thread(2).unwrap();
    context.release_mutex(mutex).unwrap();
    context.release_mutex(mutex).unwrap();
    context.set_current_thread(1).unwrap();
    assert_eq!(
        value(
            context
                .wait(&[mutex], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
}

#[test]
fn thread_termination_signals_handles_and_hands_abandoned_mutex_ownership_to_oldest_waiter() {
    let mut context = context();
    let prepared = context.prepare_thread_handle(1, false).unwrap();
    let thread = context.commit_object_handle(prepared);
    let (mutex, _) = context.create_mutex(true, false, None).unwrap();
    context.set_current_thread(2).unwrap();
    let WaitOutcome32::Pending(wait) = context
        .wait(&[mutex], WaitMode32::Any, INFINITE, false, None)
        .unwrap()
    else {
        panic!()
    };
    context.terminate_thread_object(1).unwrap();
    assert_eq!(context.wait_status(wait).unwrap().unwrap().value, 0x80);
    assert_eq!(
        value(
            context
                .wait(&[thread], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(context.thread_from_handle(thread).unwrap(), 1);
    context.release_mutex(mutex).unwrap();
    context.close_handle(thread).unwrap();
    assert_eq!(
        context.thread_from_handle(thread),
        Err(SynchronizationError32::InvalidHandle)
    );
}

#[test]
fn named_object_types_rights_and_event_notification_bindings_do_not_alias_other_waitable_objects() {
    let mut context = context();
    let name: Vec<u16> = "shared".encode_utf16().collect();
    let (mutex, _) = context.create_mutex(false, false, Some(&name)).unwrap();
    assert_eq!(
        context.create_event(false, false, false, Some(&name)),
        Err(SynchronizationError32::InvalidHandle)
    );
    assert_eq!(
        context.open_event(0x1000_0000, false, &name),
        Err(SynchronizationError32::InvalidHandle)
    );
    assert_eq!(
        context.set_event(mutex),
        Err(SynchronizationError32::AccessDenied)
    );
    assert!(context.retain_event(mutex).is_err());
    let alias = context.open_mutex(0x1000_0000, false, &name).unwrap();
    assert_ne!(alias, mutex);
    let (semaphore, _) = context.create_semaphore(1, 2, false, None).unwrap();
    assert_eq!(
        context.release_semaphore(semaphore, 2),
        Err(SynchronizationError32::TooManyPosts)
    );
    assert_eq!(
        context.release_semaphore(semaphore, 0),
        Err(SynchronizationError32::InvalidWait)
    );
    assert_eq!(context.release_semaphore(semaphore, 1).unwrap(), 1);
    assert_eq!(
        value(
            context
                .wait(&[semaphore], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        value(
            context
                .wait(&[semaphore], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        value(
            context
                .wait(&[semaphore], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    assert!(context.retain_event(semaphore).is_err());
}

#[test]
fn event_only_notifications_keep_the_zero_projection_allocation_path_with_unrelated_mutexes() {
    let mut context = context();
    context.create_mutex(false, false, None).unwrap();
    let (event, _) = context.create_event(false, false, false, None).unwrap();
    let binding = context.retain_event(event).unwrap();
    let prepared = context.prepare_signals(&[binding]).unwrap();
    assert!(prepared.objects.is_empty());
    assert_eq!(prepared.objects.capacity(), 0);
    context.commit_signals(prepared);
    assert!(context.event_is_signaled(event).unwrap());
}
