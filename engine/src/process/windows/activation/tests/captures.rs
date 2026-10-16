use super::*;
use crate::windows::user32::User32Limits32;

#[test]
fn managed_message_copies_share_one_internal_hold_and_expiry_reclaims_the_exact_context() {
    let (mut engine, _) = engine(2);
    engine
        .configure_user32(
            User32Limits32::default(),
            PageRange::new(GuestAddress(0x80000), 8).unwrap(),
        )
        .unwrap();
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    let prepared = engine.prepare_managed_activation_capture().unwrap();
    let token = prepared.token();
    let copy = token.clone();
    engine.commit_managed_activation_capture(prepared);
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
    drop(token);
    engine.collect_activation_captures().unwrap();
    assert!(engine.activation.as_ref().unwrap().native.contains(handle));
    drop(copy);
    engine.collect_activation_captures().unwrap();
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
    assert!(engine.activation.as_ref().unwrap().captures.is_empty());
}

#[test]
fn scoped_callback_entry_borrows_managed_capture_without_consuming_its_lease() {
    let (mut engine, _) = engine(2);
    engine
        .configure_user32(
            User32Limits32::default(),
            PageRange::new(GuestAddress(0x80000), 8).unwrap(),
        )
        .unwrap();
    let handle = create(&mut engine);
    engine
        .activation_call(1, Api::ActivateActCtx, &[handle, OUTPUT])
        .unwrap();
    let cookie = word(&engine, OUTPUT);
    let prepared = engine.prepare_managed_activation_capture().unwrap();
    let token = prepared.token();
    engine.commit_managed_activation_capture(prepared);
    engine
        .activation_call(1, Api::DeactivateActCtx, &[0, cookie])
        .unwrap();
    engine
        .activation_call(1, Api::ReleaseActCtx, &[handle])
        .unwrap();
    let prepared = engine.prepare_activation_entry_borrowed(handle).unwrap();
    let scope = engine.commit_activation_entry(prepared);
    assert_eq!(
        engine.activation.as_ref().unwrap().native.current(1),
        handle
    );
    let prepared = engine.prepare_activation_scope_exit(scope).unwrap();
    engine.commit_activation_scope_exit(prepared);
    assert_eq!(engine.activation.as_ref().unwrap().native.current(1), 0);
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
    drop(token);
    engine.collect_activation_captures().unwrap();
    assert!(!engine.activation.as_ref().unwrap().native.contains(handle));
}
