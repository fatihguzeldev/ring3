use super::*;

#[test]
fn queued_capture_keeps_exact_identity_and_scoped_return_unwinds_callback_frames_only() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let caller = create(&mut contexts, "Caller", false);
    let receiver = create(&mut contexts, "Receiver", false);
    let temporary = create(&mut contexts, "Temporary", false);
    activate(&mut contexts, 1, caller);
    activate(&mut contexts, 2, receiver);
    contexts.retain_hold(caller).unwrap();
    contexts.release(caller).unwrap();
    contexts.retire_thread(1);
    assert_eq!(contexts.references(caller).unwrap(), 1);
    let cookie = activate(&mut contexts, 2, caller);
    let scope = ActivationScope32::new(2, cookie, caller);
    assert_eq!(contexts.current(2), caller);
    activate(&mut contexts, 2, temporary);
    contexts.release(temporary).unwrap();
    let exit = contexts.prepare_scope_exit(scope).unwrap();
    assert_eq!(exit.disposed(), [caller, temporary]);
    let disposed = contexts.commit_scope_exit(exit);
    assert_eq!(disposed, [caller, temporary]);
    assert_eq!(contexts.current(2), receiver);
    assert!(!contexts.contains(caller));
    assert!(!contexts.contains(temporary));
    assert_eq!(contexts.references(receiver).unwrap(), 2);
}

#[test]
fn scope_receipt_rejects_reentrant_changes_and_thread_retirement_releases_all_capture_holds() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let handle = create(&mut contexts, "Captured", false);
    contexts.retain_hold(handle).unwrap();
    contexts.retain_hold(handle).unwrap();
    let cookie = activate(&mut contexts, 2, handle);
    let scope = ActivationScope32::new(2, cookie, handle);
    let stale = contexts.prepare_scope_exit(scope).unwrap();
    contexts.add_ref(handle).unwrap();
    assert_eq!(
        contexts.can_commit_scope_exit(&stale),
        Err(ActivationError32::Stale)
    );
    assert_eq!(contexts.current(2), handle);
    contexts.release(handle).unwrap();
    contexts.release(handle).unwrap();
    let retirement = contexts
        .prepare_thread_retirement(2, vec![handle, handle])
        .unwrap();
    assert_eq!(retirement.disposed(), [handle]);
    assert_eq!(contexts.commit_thread_retirement(retirement), [handle]);
    assert_eq!(contexts.current(2), 0);
    assert!(!contexts.contains(handle));
}
