use super::*;

fn wait(
    context: &mut SynchronizationContext32,
    handles: &[u32],
    mode: WaitMode32,
    timeout: u32,
) -> Result<WaitOutcome32, SynchronizationError32> {
    context.with_current_wait_objects(42, handles, |context| {
        context.wait(handles, mode, timeout, false, None)
    })
}

#[test]
fn pseudo_waits_do_not_allocate_visible_handles_bindings_or_retained_zero_timeout_objects() {
    let mut context = SynchronizationContext32::new(
        7,
        SynchronizationLimits32 {
            events: 4,
            handles: 1,
            bindings: 1,
            waits: 4,
        },
        0,
    )
    .unwrap();
    let event = context.create_event(false, true, false, None).unwrap().0;
    let next_handle = context.next_handle;
    let next_binding = context.next_binding;
    for pseudo in [u32::MAX, 0xffff_fffe] {
        assert_eq!(
            wait(&mut context, &[pseudo], WaitMode32::Any, 0).unwrap(),
            WaitOutcome32::Ready(WaitResult32 {
                value: WAIT_TIMEOUT,
                last_error: None,
            })
        );
        assert_eq!(context.objects.iter().flatten().count(), 1);
    }
    assert_eq!(
        wait(
            &mut context,
            &[event, u32::MAX, 0xffff_fffe],
            WaitMode32::All,
            0
        )
        .unwrap(),
        WaitOutcome32::Ready(WaitResult32 {
            value: WAIT_TIMEOUT,
            last_error: None,
        })
    );
    assert!(context.event_is_signaled(event).unwrap());
    assert_eq!(context.handles.len(), 1);
    assert_eq!(context.bindings.iter().flatten().count(), 0);
    assert_eq!(context.next_handle, next_handle);
    assert_eq!(context.next_binding, next_binding);
    assert_eq!(context.typed_objects, 0);
}

#[test]
fn pending_pseudo_wait_holds_are_shared_and_reclaimed_after_cancel_take_or_thread_exit() {
    let mut context =
        SynchronizationContext32::new(7, SynchronizationLimits32::default(), 0).unwrap();
    let WaitOutcome32::Pending(first) =
        wait(&mut context, &[u32::MAX], WaitMode32::Any, INFINITE).unwrap()
    else {
        panic!("live process");
    };
    let WaitOutcome32::Pending(second) =
        wait(&mut context, &[u32::MAX], WaitMode32::Any, INFINITE).unwrap()
    else {
        panic!("live process");
    };
    assert_eq!(context.objects.iter().flatten().count(), 1);
    assert_eq!(context.objects.iter().flatten().next().unwrap().bindings, 2);
    context.cancel_wait(first).unwrap();
    assert_eq!(context.objects.iter().flatten().next().unwrap().bindings, 1);
    let exit = context.prepare_process_exit(42).unwrap();
    context.commit_signals(exit);
    assert_eq!(context.take_wait(second).unwrap().unwrap().value, 0);
    assert!(context.objects.iter().all(Option::is_none));
    assert_eq!(context.typed_objects, 0);
    context.set_current_thread(2).unwrap();
    let WaitOutcome32::Pending(thread) =
        wait(&mut context, &[0xffff_fffe], WaitMode32::Any, INFINITE).unwrap()
    else {
        panic!("live current thread");
    };
    context.terminate_thread_object(2).unwrap();
    assert_eq!(
        context.wait_status(thread),
        Err(SynchronizationError32::Stale)
    );
    assert!(context.objects.iter().all(Option::is_none));
}

#[test]
fn pseudo_projection_uses_canonical_alias_identity_and_rolls_back_failed_admission() {
    let mut context =
        SynchronizationContext32::new(7, SynchronizationLimits32::default(), 0).unwrap();
    let prepared = context
        .prepare_process_handle(42, SYNCHRONIZE, false)
        .unwrap();
    let alias = context.commit_object_handle(prepared);
    let revision = context.revision();
    let next_object = context.next_object;
    assert_eq!(
        wait(&mut context, &[alias, u32::MAX], WaitMode32::All, 0),
        Err(SynchronizationError32::InvalidWait)
    );
    assert_eq!(context.revision(), revision);
    assert_eq!(context.next_object, next_object);
    assert!(
        context
            .objects
            .iter()
            .flatten()
            .all(|object| object.bindings == 0)
    );
    context.set_revision_for_test(u64::MAX);
    assert_eq!(
        wait(&mut context, &[0xffff_fffe], WaitMode32::Any, INFINITE),
        Err(SynchronizationError32::Exhausted)
    );
    assert_eq!(context.next_object, next_object);
    assert_eq!(context.objects.iter().flatten().count(), 1);
    assert!(!context.has_thread_object(1));
    assert!(context.pseudo_wait_slots.iter().all(Option::is_none));
}
