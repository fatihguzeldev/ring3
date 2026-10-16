use super::*;
#[test]
fn merged_classes_watch_observes_user_override_created_after_machine_only_handle() {
    let mut registry = context(false);
    let machine = create(
        &mut registry,
        0x80000002,
        "Software\\Classes\\Game",
        0xf003f,
    );
    set(&mut registry, machine, "Value", &[1]);
    let opened = registry
        .prepare_open(0x80000000, &units("Game"), 0x10)
        .unwrap();
    let classes = registry.commit(opened);
    let watch = registry.watch(classes, true, 4).unwrap();
    let user = create(
        &mut registry,
        0x80000001,
        "Software\\Classes\\Game",
        0xf003f,
    );
    let prepared = registry
        .prepare_set_value(user, &units("Value"), 3, &[2], 300, true)
        .unwrap();
    assert!(registry.notifies(&prepared, watch));
    registry.commit(prepared);
}
#[test]
fn atomic_create_and_default_value_notifies_last_set_subtree_watch() {
    let mut registry = context(false);
    let parent = create(&mut registry, 0x80000001, "Game", 0xf003f);
    let watch = registry.watch(parent, true, 4).unwrap();
    let prepared = registry
        .prepare_set_path(parent, &units("New"), &[], 1, &[65, 0, 0, 0], 300, true)
        .unwrap();
    assert!(registry.notifies(&prepared, watch));
    registry.commit(prepared);
}
