use super::*;

#[test]
fn named_event_reopen_ignores_creation_flags_and_preserves_case_sensitive_identity() {
    let mut context = context();
    let name: Vec<u16> = "Ready".encode_utf16().collect();
    let (first, existed) = context
        .create_event(true, true, false, Some(&name))
        .unwrap();
    assert!(!existed);
    let local: Vec<u16> = "Local\\Ready".encode_utf16().collect();
    let (second, existed) = context
        .create_event(false, false, true, Some(&local))
        .unwrap();
    assert!(existed);
    assert_ne!(first, second);
    assert!(context.handle_inheritable(second).unwrap());
    assert_eq!(
        ready(
            context
                .wait(&[first], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        ready(
            context
                .wait(&[second], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    context.reset_event(second).unwrap();
    assert!(!context.event_is_signaled(first).unwrap());
    let wrong: Vec<u16> = "ready".encode_utf16().collect();
    assert_eq!(
        context.open_event(SYNCHRONIZE, false, &wrong),
        Err(SynchronizationError32::NotFound)
    );
    let restricted = context.open_event(SYNCHRONIZE, false, &name).unwrap();
    assert_eq!(
        context.set_event(restricted),
        Err(SynchronizationError32::AccessDenied)
    );
    assert_eq!(
        context.retain_event(restricted),
        Err(SynchronizationError32::AccessDenied)
    );
    context.close_handle(first).unwrap();
    context.close_handle(second).unwrap();
    context.close_handle(restricted).unwrap();
    assert_eq!(
        context.open_event(SYNCHRONIZE, false, &name),
        Err(SynchronizationError32::NotFound)
    );
}

#[test]
fn duplicate_handles_share_state_while_closed_and_reused_slots_keep_stale_handles_invalid() {
    let mut context = context();
    let (first, _) = context.create_event(false, false, false, None).unwrap();
    let prepared = context
        .prepare_duplicate(first, 0, true, true, true, true)
        .unwrap();
    let second = prepared.handle().unwrap();
    context.commit_duplicate(prepared);
    assert_eq!(
        context.event_is_signaled(first),
        Err(SynchronizationError32::InvalidHandle)
    );
    context.set_event(second).unwrap();
    assert_eq!(
        ready(
            context
                .wait(&[second], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        ready(
            context
                .wait(&[second], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    context.close_handle(second).unwrap();
    assert_eq!(
        context.close_handle(second),
        Err(SynchronizationError32::InvalidHandle)
    );
    let (third, _) = context.create_event(true, true, false, None).unwrap();
    assert!(third > second);
    assert_eq!(
        context.set_event(second),
        Err(SynchronizationError32::InvalidHandle)
    );
}

#[test]
fn retained_notification_binding_survives_handle_close_and_batch_replacement_is_neutral_on_failure()
{
    let mut context = context();
    let name: Vec<u16> = "Audio".encode_utf16().collect();
    let (handle, _) = context
        .create_event(false, false, false, Some(&name))
        .unwrap();
    let binding = context.retain_event(handle).unwrap();
    context.close_handle(handle).unwrap();
    let reopened = context.open_event(EVENT_ALL_ACCESS, false, &name).unwrap();
    let revision = context.revision();
    assert!(matches!(
        context.prepare_event_bindings(&[reopened, 0], &[binding]),
        Err(SynchronizationError32::InvalidHandle)
    ));
    assert_eq!(context.revision(), revision);
    context.validate_event_binding(binding).unwrap();
    let replacement = context
        .prepare_event_bindings(&[reopened, reopened], &[binding])
        .unwrap();
    assert_eq!(replacement.bindings().len(), 2);
    let bindings = context.commit_event_bindings(replacement);
    assert_eq!(
        context.release_event(binding),
        Err(SynchronizationError32::Stale)
    );
    let signals = context.prepare_signals(&bindings).unwrap();
    context.commit_signals(signals);
    assert_eq!(
        ready(
            context
                .wait(&[reopened], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
    assert_eq!(
        ready(
            context
                .wait(&[reopened], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        WAIT_TIMEOUT
    );
    let release = context.prepare_event_bindings(&[], &bindings).unwrap();
    context.commit_event_bindings(release);
    assert_eq!(
        context.validate_event_binding(bindings[0]),
        Err(SynchronizationError32::Stale)
    );
}

#[test]
fn exhausted_revision_rejects_mutation_without_consuming_signal_binding_or_handle() {
    let mut context = context();
    let (handle, _) = context.create_event(false, true, false, None).unwrap();
    let binding = context.retain_event(handle).unwrap();
    context.revision = u64::MAX;
    assert_eq!(
        context.set_event(handle),
        Err(SynchronizationError32::Exhausted)
    );
    assert_eq!(
        context.close_handle(handle),
        Err(SynchronizationError32::Exhausted)
    );
    assert_eq!(
        context.release_event(binding),
        Err(SynchronizationError32::Exhausted)
    );
    assert!(matches!(
        context.prepare_signals(&[binding]),
        Err(SynchronizationError32::Exhausted)
    ));
    assert!(context.event_is_signaled(handle).unwrap());
    context.validate_event_binding(binding).unwrap();
    context.revision = 4;
    assert_eq!(
        ready(
            context
                .wait(&[handle], WaitMode32::Any, 0, false, None)
                .unwrap()
        ),
        0
    );
}

#[test]
fn epoch_reset_invalidates_binding_wait_and_handle_without_reusing_guest_values() {
    let mut context = context();
    let (handle, _) = context.create_event(false, false, false, None).unwrap();
    let binding = context.retain_event(handle).unwrap();
    let wait = pending(
        context
            .wait(&[handle], WaitMode32::Any, INFINITE, false, None)
            .unwrap(),
    );
    context.reset(8, 100).unwrap();
    assert_eq!(
        context.wait_status(wait),
        Err(SynchronizationError32::Stale)
    );
    assert!(matches!(
        context.prepare_signals(&[binding]),
        Err(SynchronizationError32::Stale)
    ));
    assert_eq!(
        context.set_event(handle),
        Err(SynchronizationError32::InvalidHandle)
    );
    let (new, _) = context.create_event(false, false, false, None).unwrap();
    assert!(new > handle);
}

#[test]
fn io_acceptance_retains_and_resets_events_in_one_failure_neutral_receipt() {
    let mut context = context();
    let (event, _) = context.create_event(true, true, false, None).unwrap();
    let revision = context.revision();
    assert!(
        context
            .prepare_io_events(&[event], &[], &[0xffff_fffc])
            .is_err()
    );
    assert_eq!(context.revision(), revision);
    let prepared = context
        .prepare_io_events(&[event], &[], &[event, event])
        .unwrap();
    let bindings = context.commit_event_bindings(prepared);
    assert_eq!(bindings.len(), 1);
    assert!(matches!(
        context
            .wait(&[event], WaitMode32::Any, 0, false, None)
            .unwrap(),
        WaitOutcome32::Ready(WaitResult32 {
            value: WAIT_TIMEOUT,
            ..
        })
    ));
    context.close_handle(event).unwrap();
    context.validate_event_binding(bindings[0]).unwrap();
    let signal = context.prepare_signals(&[bindings[0]]).unwrap();
    context.commit_signals(signal);
    context.release_event(bindings[0]).unwrap();
}

#[test]
fn immediate_io_signal_preflights_all_handles_before_waking_waiters() {
    let mut context=context();
    let (first,_)=context.create_event(false,false,false,None).unwrap();
    let wait=match context.wait(&[first],WaitMode32::Any,INFINITE,false,None).unwrap(){WaitOutcome32::Pending(wait)=>wait,_=>panic!("pending")};
    let revision=context.revision();
    assert!(context.prepare_signal_handles(&[first,0xffff_fffc]).is_err());
    assert_eq!(context.revision(),revision);
    assert_eq!(context.wait_status(wait).unwrap(),None);
    let prepared=context.prepare_signal_handles(&[first]).unwrap();
    context.commit_signals(prepared);
    assert_eq!(context.take_wait(wait).unwrap().unwrap().value,0);
}
