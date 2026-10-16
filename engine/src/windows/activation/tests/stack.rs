use super::*;

#[test]
fn frames_retain_contexts_and_get_current_can_acquire_a_new_external_reference() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let handle = create(&mut contexts, "Ring3.Ref", false);
    let cookie = activate(&mut contexts, 1, handle);
    assert_eq!(contexts.references(handle).unwrap(), 2);
    assert!(!contexts.release(handle).unwrap());
    assert_eq!(
        contexts.release(handle),
        Err(ActivationError32::InvalidHandle)
    );
    assert_eq!(contexts.current(1), handle);
    contexts.add_ref(contexts.current(1)).unwrap();
    let pop = contexts.prepare_deactivate(1, 0, cookie).unwrap();
    contexts.commit_deactivate(pop);
    assert_eq!(contexts.references(handle).unwrap(), 1);
    assert!(contexts.release(handle).unwrap());
    assert!(!contexts.contains(handle));
}

#[test]
fn null_activation_blocks_lower_frames_and_lookup_falls_back_to_process_default() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let default = create(&mut contexts, "Ring3.Default", true);
    let isolated = create(&mut contexts, "Ring3.Isolated", false);
    let lower = activate(&mut contexts, 1, isolated);
    let upper = activate(&mut contexts, 1, 0);
    assert_eq!(contexts.current(1), 0);
    assert_eq!(contexts.current(2), 0);
    assert_eq!(
        contexts
            .active_com_redirection(1, CLSID)
            .unwrap()
            .unwrap()
            .context,
        default
    );
    assert!(matches!(
        contexts.prepare_deactivate(1, 0, lower),
        Err(ActivationError32::EarlyDeactivation)
    ));
    assert!(matches!(
        contexts.prepare_deactivate(2, 0, upper),
        Err(ActivationError32::InvalidDeactivation)
    ));
    assert_eq!(
        ActivationError32::EarlyDeactivation.exception(),
        Some(0xc015000f)
    );
    assert_eq!(
        ActivationError32::InvalidDeactivation.exception(),
        Some(0xc0150010)
    );
    let pop = contexts.prepare_deactivate(1, 1, lower).unwrap();
    contexts.commit_deactivate(pop);
    assert_eq!(contexts.current(1), 0);
    assert!(matches!(
        contexts.prepare_deactivate(1, 0, upper),
        Err(ActivationError32::InvalidDeactivation)
    ));
}

#[test]
fn prepared_publication_rejects_stale_state_and_recycles_capacity_without_handle_reuse() {
    let limits = ActivationConfig32 {
        contexts: 1,
        frames: 1,
        ..Default::default()
    };
    let mut contexts = ActivationContexts32::new(limits).unwrap();
    let first = create(&mut contexts, "Ring3.First", false);
    let stale = contexts.prepare_activate(1, first).unwrap();
    contexts.add_ref(first).unwrap();
    assert_eq!(
        contexts.can_commit_activate(&stale),
        Err(ActivationError32::Stale)
    );
    contexts.release(first).unwrap();
    contexts.release(first).unwrap();
    let second = create(&mut contexts, "Ring3.Second", false);
    assert_ne!(first, second);
    let cookie = activate(&mut contexts, 1, second);
    assert!(matches!(
        contexts.prepare_activate(2, second),
        Err(ActivationError32::NotEnoughMemory)
    ));
    let pop = contexts.prepare_deactivate(1, 0, cookie).unwrap();
    contexts.commit_deactivate(pop);
    assert_ne!(activate(&mut contexts, 2, second), cookie);
    contexts.check_retire_thread(2).unwrap();
    contexts.retire_thread(2);
    assert_eq!(contexts.references(second).unwrap(), 1);
}

#[test]
fn inherited_context_owns_its_reference_and_resolved_server_uses_virtual_assembly_directory() {
    let mut contexts = ActivationContexts32::new(ActivationConfig32::default()).unwrap();
    let handle = create(&mut contexts, "Ring3.Inherit", false);
    activate(&mut contexts, 1, handle);
    let inherited = contexts.prepare_inherit(1, 2).unwrap().unwrap();
    contexts.commit_activate(inherited);
    contexts.retire_thread(1);
    assert_eq!(contexts.current(2), handle);
    assert_eq!(
        contexts
            .active_com_redirection(2, CLSID)
            .unwrap()
            .unwrap()
            .server,
        "C:\\app\\private.dll".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        contexts
            .active_dll_redirection(2, &"PRIVATE.DLL".encode_utf16().collect::<Vec<_>>())
            .unwrap(),
        Some("C:\\app\\private.dll".encode_utf16().collect::<Vec<_>>())
    );
}
