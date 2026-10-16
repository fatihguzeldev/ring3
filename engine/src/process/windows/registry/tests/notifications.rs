use super::*;
use crate::windows::synchronization::SynchronizationLimits32;
fn watch(engine: &mut EngineInstance, handle: u32, event: u32, filter: u32) {
    let binding = engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .retain_event(event)
        .unwrap();
    let runtime = engine.registry.as_mut().unwrap();
    let watch = runtime.native.watch(handle, true, filter).unwrap();
    runtime.watches[0] = Some(super::super::notifications::Registration32 {
        id: 1,
        handle,
        thread: 1,
        agnostic: false,
        watch,
        event: Some(binding),
        ready: false,
    });
}
#[test]
fn notifications_are_one_shot_and_failed_create_output_does_not_signal() {
    let mut engine = engine();
    engine.configure_cpu_clock(10_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    let handle = create(&mut engine);
    let event = engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .create_event(true, false, false, None)
        .unwrap()
        .0;
    watch(&mut engine, handle, event, 1);
    put(&mut engine, 0x2000, b"Child\0");
    assert_eq!(
        engine.registry_call(
            Api::RegCreateKeyExA,
            &[handle, 0x2000, 0, 0, 0, 0xf003f, 0, 0x2100, 0xfffffff0],
            200
        ),
        Err(RegistryError32::NoAccess)
    );
    assert_eq!(
        engine
            .synchronization
            .as_ref()
            .unwrap()
            .context()
            .event_is_signaled(event),
        Ok(false)
    );
    assert_eq!(
        engine.registry_call(
            Api::RegCreateKeyExA,
            &[handle, 0x2000, 0, 0, 0, 0xf003f, 0, 0x2100, 0],
            200
        ),
        Ok(0)
    );
    assert_eq!(
        engine
            .synchronization
            .as_ref()
            .unwrap()
            .context()
            .event_is_signaled(event),
        Ok(true)
    );
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .reset_event(event)
        .unwrap();
    put(&mut engine, 0x2000, b"Other\0");
    engine
        .registry_call(
            Api::RegCreateKeyExA,
            &[handle, 0x2000, 0, 0, 0, 0xf003f, 0, 0x2100, 0],
            300,
        )
        .unwrap();
    assert_eq!(
        engine
            .synchronization
            .as_ref()
            .unwrap()
            .context()
            .event_is_signaled(event),
        Ok(false)
    );
}
#[test]
fn closing_watched_numeric_key_signals_and_releases_binding() {
    let mut engine = engine();
    engine.configure_cpu_clock(10_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    let handle = create(&mut engine);
    let event = engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .create_event(true, false, false, None)
        .unwrap()
        .0;
    watch(&mut engine, handle, event, 4);
    engine
        .registry_call(Api::RegCloseKey, &[handle], 200)
        .unwrap();
    assert_eq!(
        engine
            .synchronization
            .as_ref()
            .unwrap()
            .context()
            .event_is_signaled(event),
        Ok(true)
    );
    assert!(
        engine
            .registry
            .as_ref()
            .unwrap()
            .watches
            .iter()
            .all(Option::is_none)
    );
}

#[test]
fn issuing_thread_exit_signals_bound_watch_and_preserves_thread_agnostic_watch() {
    let mut engine = engine();
    engine.configure_cpu_clock(10_000_000, 0).unwrap();
    engine
        .configure_synchronization(SynchronizationLimits32::default())
        .unwrap();
    let handle = create(&mut engine);
    let sync = engine.synchronization.as_mut().unwrap().context_mut();
    let first = sync.create_event(true, false, false, None).unwrap().0;
    let second = sync.create_event(true, false, false, None).unwrap().0;
    watch(&mut engine, handle, first, 4);
    let binding = engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .retain_event(second)
        .unwrap();
    let runtime = engine.registry.as_mut().unwrap();
    let specification = runtime.native.watch(handle, false, 0x10000004).unwrap();
    runtime.watches[1] = Some(super::super::notifications::Registration32 {
        id: 2,
        handle,
        thread: 1,
        agnostic: true,
        watch: specification,
        event: Some(binding),
        ready: false,
    });
    let bindings = runtime.thread_exit_bindings(1).unwrap();
    assert_eq!(bindings.len(), 1);
    let prepared = engine
        .synchronization
        .as_ref()
        .unwrap()
        .context()
        .prepare_signals_and_release(&bindings)
        .unwrap();
    engine.registry.as_mut().unwrap().commit_thread_exit(1);
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .commit_signals_and_release(prepared);
    let sync = engine.synchronization.as_ref().unwrap().context();
    assert_eq!(sync.event_is_signaled(first), Ok(true));
    assert_eq!(sync.event_is_signaled(second), Ok(false));
    put(&mut engine, 0x2300, &[1]);
    engine
        .registry_call(Api::RegSetValueExW, &[handle, 0, 0, 3, 0x2300, 1], 300)
        .unwrap();
    assert_eq!(
        engine
            .synchronization
            .as_ref()
            .unwrap()
            .context()
            .event_is_signaled(second),
        Ok(true)
    );
}
