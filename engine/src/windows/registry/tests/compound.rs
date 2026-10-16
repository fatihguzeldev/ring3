use super::*;
#[test]
fn internal_set_path_does_not_require_an_extra_numeric_key_handle() {
    let config = RegistryConfig32 {
        limits: RegistryLimits32 {
            handles: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut registry = Registry32::new(7, config).unwrap();
    let handle = create(&mut registry, 0x80000001, "Game", 0xf003f);
    let prepared = registry
        .prepare_set_path(handle, &units("Child"), &[], 1, &[65, 0, 0, 0], 300, true)
        .unwrap();
    registry.commit(prepared);
    assert_eq!(
        registry
            .query_subkey_value(handle, &units("Child"), 0, &[])
            .unwrap()
            .bytes,
        [65, 0, 0, 0]
    );
}
#[test]
fn host_seed_does_not_require_an_extra_numeric_key_handle() {
    let config = RegistryConfig32 {
        limits: RegistryLimits32 {
            handles: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut registry = Registry32::new(7, config).unwrap();
    let _handle = create(&mut registry, 0x80000001, "Game", 0xf003f);
    let prepared = registry
        .prepare_seed(RegistrySeed32 {
            root: 0x80000001,
            view: RegistryView32::View32,
            path: &units("Seed"),
            class: &[],
            name: &[],
            kind: 3,
            bytes: &[9],
            modified: 100,
            volatile: false,
        })
        .unwrap();
    registry.commit(prepared);
    assert_eq!(
        registry
            .query_subkey_value(0x80000001, &units("Seed"), 0, &[])
            .unwrap()
            .bytes,
        [9]
    );
}
