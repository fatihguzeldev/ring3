use super::*;
use crate::windows::synchronization::{SynchronizationApi32 as SyncApi, WaitOutcome32};

#[test]
fn waitable_timer_freezes_registration_context_and_cancel_releases_queued_capture() {
    let (mut engine, _) = engine(2);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    let (prepared, _) = engine
        .synchronization()
        .unwrap()
        .prepare_timer(false, false, None)
        .unwrap();
    let timer = engine
        .synchronization_mut()
        .unwrap()
        .commit_object_handle(prepared);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    write(&mut engine, INPUT + 0x1c0, &(-10_000i64).to_le_bytes());
    assert!(matches!(
        engine
            .execute_timer_call(
                SyncApi::SetWaitableTimer,
                &[timer, INPUT + 0x1c0, 0, 0x401000, 42, 0]
            )
            .unwrap(),
        WaitOutcome32::Ready(_)
    ));
    let (object, generation, _) = engine
        .synchronization()
        .unwrap()
        .timer_identity(timer)
        .unwrap();
    let captured = engine
        .waitable_timer_activation(object, generation)
        .unwrap();
    assert_eq!(captured.handle(), handle);
    drop(captured);
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    assert_eq!(engine.activation.as_ref().unwrap().native.current(1), 0);
    engine.observe_cpu_clock(1_000_000).unwrap();
    assert!(engine.has_pending_thread_apc(1));
    assert!(engine.activation.as_ref().unwrap().native.contains(handle));
    engine
        .execute_timer_call(SyncApi::CancelWaitableTimer, &[timer])
        .unwrap();
    assert!(!engine.has_pending_thread_apc(1));
    assert!(
        engine
            .waitable_timer_activation(object, generation)
            .is_none()
    );
    engine.collect_activation_captures().unwrap();
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}

#[test]
fn running_waitable_callback_keeps_captured_context_after_registration_cancel() {
    let (mut engine, _) = engine(2);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    let (prepared, _) = engine
        .synchronization()
        .unwrap()
        .prepare_timer(false, false, None)
        .unwrap();
    let timer = engine
        .synchronization_mut()
        .unwrap()
        .commit_object_handle(prepared);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    write(&mut engine, INPUT + 0x1c0, &(-10_000i64).to_le_bytes());
    engine
        .execute_timer_call(
            SyncApi::SetWaitableTimer,
            &[timer, INPUT + 0x1c0, 10, 0x401000, 42, 0],
        )
        .unwrap();
    let (object, generation, _) = engine
        .synchronization()
        .unwrap()
        .timer_identity(timer)
        .unwrap();
    let callback = engine
        .waitable_timer_activation(object, generation)
        .unwrap();
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    let prepared = engine
        .prepare_activation_entry_borrowed(callback.handle())
        .unwrap();
    let scope = engine.commit_activation_entry(prepared);
    engine
        .execute_timer_call(SyncApi::CancelWaitableTimer, &[timer])
        .unwrap();
    engine.collect_activation_captures().unwrap();
    assert_eq!(
        engine.activation.as_ref().unwrap().native.current(1),
        handle
    );
    let prepared = engine.prepare_activation_scope_exit(scope).unwrap();
    engine.commit_activation_scope_exit(prepared);
    assert!(engine.activation.as_ref().unwrap().native.contains(handle));
    drop(callback);
    engine.collect_activation_captures().unwrap();
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}

fn sole_timer_capture() -> (EngineInstance, u32, u32, u32) {
    let (mut engine, _) = engine(2);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(crate::windows::synchronization::SynchronizationLimits32 {
            events: 1,
            handles: 8,
            bindings: 8,
            waits: 4,
        })
        .unwrap();
    let (prepared, _) = engine
        .synchronization()
        .unwrap()
        .prepare_timer(false, false, None)
        .unwrap();
    let timer = engine
        .synchronization_mut()
        .unwrap()
        .commit_object_handle(prepared);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    write(&mut engine, INPUT + 0x1c0, &(-10_000i64).to_le_bytes());
    engine
        .execute_timer_call(
            SyncApi::SetWaitableTimer,
            &[timer, INPUT + 0x1c0, 0, 0x401000, 42, 0],
        )
        .unwrap();
    (engine, timer, handle, cookie)
}

#[test]
fn waitable_timer_rearming_at_one_object_capacity_replaces_capture_without_a_lifetime_quota() {
    let (mut engine, timer, handle, cookie) = sole_timer_capture();
    assert!(engine.user32.is_none());
    for _ in 0..64 {
        let before = engine
            .synchronization()
            .unwrap()
            .timer_identity(timer)
            .unwrap();
        let memory_version = engine.memory().unwrap().transaction_version();
        assert!(
            engine
                .execute_timer_call(
                    SyncApi::SetWaitableTimer,
                    &[timer, u32::MAX, 0, 0x401000, 42, 0]
                )
                .is_err()
        );
        assert_eq!(
            engine
                .synchronization()
                .unwrap()
                .timer_identity(timer)
                .unwrap(),
            before
        );
        assert_eq!(
            engine.memory().unwrap().transaction_version(),
            memory_version
        );
        engine
            .execute_timer_call(
                SyncApi::SetWaitableTimer,
                &[timer, INPUT + 0x1c0, 0, 0x401000, 42, 0],
            )
            .unwrap();
        engine.collect_activation_captures().unwrap();
        assert_eq!(engine.activation.as_ref().unwrap().captures.len(), 1);
        assert_eq!(
            engine
                .activation
                .as_ref()
                .unwrap()
                .native
                .references(handle)
                .unwrap(),
            3
        );
    }
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    engine
        .execute_timer_call(SyncApi::CancelWaitableTimer, &[timer])
        .unwrap();
    engine.collect_activation_captures().unwrap();
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
    assert!(
        engine
            .synchronization()
            .unwrap()
            .timer_identity(timer)
            .is_ok()
    );
}

#[test]
fn waitable_timer_issuer_exit_retires_capture_while_an_open_timer_remains_valid() {
    let (mut engine, timer, handle, cookie) = sole_timer_capture();
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    let exit = engine
        .synchronization()
        .unwrap()
        .prepare_thread_exit(1)
        .unwrap();
    engine.synchronization_mut().unwrap().commit_signals(exit);
    engine.collect_activation_captures().unwrap();
    assert!(
        engine
            .synchronization()
            .unwrap()
            .timer_identity(timer)
            .is_ok()
    );
    assert_eq!(
        engine.synchronization().unwrap().next_timer_deadline(),
        None
    );
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}

#[test]
fn one_shot_timer_capture_survives_queued_and_running_callback_then_retires_without_close() {
    let (mut engine, timer, handle, cookie) = sole_timer_capture();
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    engine.observe_cpu_clock(1_000_000).unwrap();
    engine.collect_activation_captures().unwrap();
    assert!(engine.has_pending_thread_apc(1));
    assert!(engine.activation.as_ref().unwrap().native.contains(handle));
    let (object, generation, _) = engine
        .synchronization()
        .unwrap()
        .timer_identity(timer)
        .unwrap();
    let running = engine
        .waitable_timer_activation(object, generation)
        .unwrap();
    let delivery = engine
        .synchronization()
        .unwrap()
        .prepare_timer_apc_delivery(object, generation, 1, false)
        .unwrap();
    engine.cancel_thread_timer_apcs(object);
    engine
        .synchronization_mut()
        .unwrap()
        .commit_signals(delivery);
    engine.collect_activation_captures().unwrap();
    assert!(engine.activation.as_ref().unwrap().native.contains(handle));
    drop(running);
    engine.collect_activation_captures().unwrap();
    assert!(
        engine
            .synchronization()
            .unwrap()
            .timer_identity(timer)
            .is_ok()
    );
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}
