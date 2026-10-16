use super::*;
#[test]
fn merged_classes_prefers_user_values_and_enumerates_both_names_once() {
    let mut registry = context(false);
    let machine = create(
        &mut registry,
        0x80000002,
        "Software\\Classes\\Example",
        0xf003f,
    );
    set(&mut registry, machine, "same", &[1]);
    set(&mut registry, machine, "machine", &[2]);
    let user = create(
        &mut registry,
        0x80000001,
        "Software\\Classes\\example",
        0xf003f,
    );
    set(&mut registry, user, "SAME", &[3]);
    set(&mut registry, user, "user", &[4]);
    let prepared = registry
        .prepare_open(0x80000000, &units("EXAMPLE"), 1)
        .unwrap();
    let merged = registry.commit(prepared);
    assert_eq!(
        registry.query_value(merged, &units("same")).unwrap().bytes,
        [3]
    );
    assert_eq!(registry.query_info(merged).unwrap().values, 3);
    assert_eq!(
        registry.enumerate_value(merged, 3).err(),
        Some(RegistryError32::NoMoreItems)
    );
    assert!(!registry.can_flush(merged).unwrap());
}
#[test]
fn failed_capacity_is_neutral_and_old_save_ack_cannot_erase_a_new_value() {
    let mut registry = context(false);
    let key = create(&mut registry, 0x80000001, "Game", 0xf003f);
    set(&mut registry, key, "", &[1]);
    let id = (0..registry.change_count())
        .find_map(|index| {
            let change = registry.change(index as u32).unwrap().unwrap();
            (change.kind == RegistryChangeKind32::SetValue).then_some(change.id)
        })
        .unwrap();
    set(&mut registry, key, "", &[2]);
    assert_eq!(
        registry.acknowledge_change(id),
        Err(RegistryError32::StaleReceipt)
    );
    let before = registry.revision();
    let huge = vec![0; registry.config.limits.bytes as usize];
    assert!(
        registry
            .prepare_set_value(key, &[], 3, &huge, 300, true)
            .is_err()
    );
    assert_eq!(registry.revision(), before);
    assert_eq!(registry.query_value(key, &[]).unwrap().bytes, [2]);
    while let Some(change) = registry.change(0).unwrap() {
        let id = change.id;
        registry.acknowledge_change(id).unwrap();
    }
    assert!(registry.can_flush(key).unwrap());
}

#[test]
fn copying_tree_merges_destination_values_and_keeps_source_immutable() {
    let mut registry = context(false);
    let source = create(&mut registry, 0x80000001, "Source", 0xf003f);
    set(&mut registry, source, "Value", &[1]);
    let child = create(&mut registry, source, "Child", 0xf003f);
    set(&mut registry, child, "Value", &[2]);
    let destination = create(&mut registry, 0x80000001, "Destination", 0xf003f);
    set(&mut registry, destination, "Other", &[3]);
    let prepared = registry
        .prepare_copy_tree(source, &[], destination, 400)
        .unwrap();
    registry.commit(prepared);
    assert_eq!(
        registry
            .query_value(destination, &units("Value"))
            .unwrap()
            .bytes,
        [1]
    );
    assert_eq!(
        registry
            .query_value(destination, &units("Other"))
            .unwrap()
            .bytes,
        [3]
    );
    assert_eq!(
        registry
            .query_subkey_value(destination, &units("Child"), 0, &units("Value"))
            .unwrap()
            .bytes,
        [2]
    );
    assert_eq!(
        registry.query_value(source, &units("Value")).unwrap().bytes,
        [1]
    );
}

#[test]
fn copied_values_in_existing_volatile_child_never_enter_persistence_export() {
    let mut registry = context(false);
    let source = create(&mut registry, 0x80000001, "Source", 0xf003f);
    let source_child = create(&mut registry, source, "Child", 0xf003f);
    set(&mut registry, source_child, "Value", &[1]);
    let destination = create(&mut registry, 0x80000001, "Destination", 0xf003f);
    let prepared = registry
        .prepare_create(destination, &units("Child"), 0xf003f, &[], true, 200, true)
        .unwrap();
    let child = registry.commit(prepared);
    while let Some(change) = registry.change(0).unwrap() {
        let id = change.id;
        registry.acknowledge_change(id).unwrap();
    }
    let prepared = registry
        .prepare_copy_tree(source, &[], destination, 300)
        .unwrap();
    registry.commit(prepared);
    assert_eq!(
        registry.query_value(child, &units("Value")).unwrap().bytes,
        [1]
    );
    assert_eq!(registry.change_count(), 0);
}
