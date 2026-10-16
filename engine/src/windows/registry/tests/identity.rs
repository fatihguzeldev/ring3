use super::*;
#[test]
fn deleted_open_key_cannot_resolve_a_recreated_case_alias_and_hkcu_matches_hku_sid() {
    let mut registry = context(false);
    let old = create(&mut registry, 0x80000001, "Software\\Game", 0xf003f);
    set(&mut registry, old, "Score", &[7]);
    let opened = registry
        .prepare_open(0x80000003, &units("S-1-5-21-0-0-0-1000\\software\\GAME"), 1)
        .unwrap();
    let alias = registry.commit(opened);
    assert_eq!(
        registry.query_value(alias, &units("score")).unwrap().bytes,
        [7]
    );
    let deleted = registry
        .prepare_delete_key(0x80000001, &units("Software\\Game"), 0, false, false, 300)
        .unwrap();
    registry.commit(deleted);
    let new = create(&mut registry, 0x80000001, "software\\game", 0xf003f);
    set(&mut registry, new, "Score", &[9]);
    assert_eq!(
        registry.query_value(old, &units("Score")).err(),
        Some(RegistryError32::KeyDeleted)
    );
    assert_eq!(
        registry.query_value(alias, &units("Score")).err(),
        Some(RegistryError32::KeyDeleted)
    );
    assert_eq!(
        registry.query_value(new, &units("score")).unwrap().bytes,
        [9]
    );
    registry.close(old).unwrap();
    registry.close(alias).unwrap();
}
#[test]
fn explicit_distinct_views_and_default_32bit_alias_profile_are_different() {
    for separate in [false, true] {
        let mut registry = context(separate);
        let a = create(&mut registry, 0x80000002, "Software\\Game", 0xf003f | 0x200);
        set(&mut registry, a, "Value", &[32]);
        let b = create(&mut registry, 0x80000002, "Software\\Game", 0xf003f | 0x100);
        set(&mut registry, b, "Value", &[64]);
        assert_eq!(
            registry.query_value(a, &units("Value")).unwrap().bytes,
            if separate { &[32][..] } else { &[64][..] }
        );
    }
}

#[test]
fn predefined_override_pins_identity_after_numeric_handle_close_and_restores_default() {
    let mut registry = context(false);
    let temporary = create(&mut registry, 0x80000001, "Temporary", 0xf003f);
    set(&mut registry, temporary, "Value", &[8]);
    registry.override_predefined(0x80000000, temporary).unwrap();
    registry.close(temporary).unwrap();
    assert_eq!(
        registry
            .query_value(0x80000000, &units("value"))
            .unwrap()
            .bytes,
        [8]
    );
    registry.override_predefined(0x80000000, 0).unwrap();
    assert_eq!(
        registry.query_value(0x80000000, &units("value")).err(),
        Some(RegistryError32::FileNotFound)
    );
}

#[test]
fn audit_prepared_registry_change_cannot_cross_a_session_with_the_same_revision() {
    let mut first = Registry32::new(7, RegistryConfig32::default()).unwrap();
    let second = Registry32::new(8, RegistryConfig32::default()).unwrap();
    assert_eq!(first.revision(), second.revision());
    let prepared = first
        .prepare_create(
            0x80000001,
            &units("Software\\Foreign"),
            0xf003f,
            &[],
            false,
            100,
            true,
        )
        .unwrap();
    assert_eq!(
        second.can_commit(&prepared),
        Err(RegistryError32::StaleReceipt)
    );
    assert_eq!(second.change_count(), 0);
    assert_eq!(second.revision(), first.revision());
    assert_eq!(first.can_commit(&prepared), Ok(()));
    let handle = first.commit(prepared);
    assert!(first.query_info(handle).is_ok());
}
