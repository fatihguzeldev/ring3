use super::*;

#[test]
fn queued_capture_outlives_the_source_stack_and_scope_return_reclaims_its_mapping() {
    let (mut engine, _) = engine(2);
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    let capture = engine.prepare_activation_capture().unwrap();
    let retained = engine.commit_activation_capture(capture);
    assert_eq!(retained, handle);
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    assert_eq!(
        engine
            .activation
            .as_ref()
            .unwrap()
            .native
            .references(handle)
            .unwrap(),
        1
    );
    let entry = engine.prepare_activation_entry(Some(retained)).unwrap();
    let scope = engine.commit_activation_entry(entry);
    assert_eq!(
        engine.activation.as_ref().unwrap().native.current(1),
        handle
    );
    let exit = engine.prepare_activation_scope_exit(scope).unwrap();
    engine.commit_activation_scope_exit(exit);
    assert_eq!(engine.activation.as_ref().unwrap().native.current(1), 0);
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}
