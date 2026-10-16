use super::*;
#[test]
fn invalid_secondary_create_output_does_not_create_key_or_publish_primary_output() {
    let mut engine = engine();
    put(&mut engine, 0x2000, b"Software\\Missing\0");
    put(&mut engine, 0x2100, &0xfeedbeefu32.to_le_bytes());
    let revision = engine.registry.as_ref().unwrap().native.revision();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert_eq!(
        engine.registry_call(
            Api::RegCreateKeyExA,
            &[0x80000001, 0x2000, 0, 0, 0, 0xf003f, 0, 0x2100, 0xfffffff0],
            100
        ),
        Err(RegistryError32::NoAccess)
    );
    assert_eq!(word(&engine, 0x2100), 0xfeedbeef);
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    assert_eq!(
        engine.registry.as_ref().unwrap().native.revision(),
        revision
    );
    assert_eq!(
        engine
            .registry
            .as_mut()
            .unwrap()
            .native
            .prepare_open(
                0x80000001,
                &"Software\\Missing".encode_utf16().collect::<Vec<_>>(),
                1
            )
            .err(),
        Some(RegistryError32::FileNotFound)
    );
}
#[test]
fn query_fault_is_atomic_across_type_size_and_data_and_readonly_handles_cannot_set() {
    let mut engine = engine();
    let handle = create(&mut engine);
    put(&mut engine, 0x2300, &[1, 2, 3, 4]);
    engine
        .registry_call(Api::RegSetValueExW, &[handle, 0, 0, 3, 0x2300, 4], 200)
        .unwrap();
    put(&mut engine, 0x2500, &4u32.to_le_bytes());
    put(&mut engine, 0x2504, &0xfeedbeefu32.to_le_bytes());
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert_eq!(
        engine.registry_call(
            Api::RegQueryValueExW,
            &[handle, 0, 0, 0x2504, 0xfffffff0, 0x2500],
            200
        ),
        Err(RegistryError32::NoAccess)
    );
    assert_eq!(word(&engine, 0x2500), 4);
    assert_eq!(word(&engine, 0x2504), 0xfeedbeef);
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    let prepared = engine
        .registry
        .as_mut()
        .unwrap()
        .native
        .prepare_open(handle, &[], 1)
        .unwrap();
    let readonly = engine.registry.as_mut().unwrap().native.commit(prepared);
    assert_eq!(
        engine.registry_call(Api::RegSetValueExW, &[readonly, 0, 0, 3, 0x2300, 4], 300),
        Err(RegistryError32::AccessDenied)
    );
}

#[test]
fn set_key_value_creates_subkey_and_delete_tree_respects_supplied_handle_grants() {
    let mut engine = engine();
    let handle = create(&mut engine);
    put(&mut engine, 0x2200, b"New\0");
    put(&mut engine, 0x2300, &[7]);
    assert_eq!(
        engine.registry_call(
            Api::RegSetKeyValueA,
            &[handle, 0x2200, 0, 3, 0x2300, 1],
            200
        ),
        Ok(0)
    );
    assert_eq!(
        engine
            .registry
            .as_ref()
            .unwrap()
            .native
            .query_subkey_value(handle, &"New".encode_utf16().collect::<Vec<_>>(), 0, &[])
            .unwrap()
            .bytes,
        [7]
    );
    let prepared = engine
        .registry
        .as_mut()
        .unwrap()
        .native
        .prepare_open(handle, &[], 1)
        .unwrap();
    let readonly = engine.registry.as_mut().unwrap().native.commit(prepared);
    let revision = engine.registry.as_ref().unwrap().native.revision();
    assert_eq!(
        engine.registry_call(Api::RegDeleteTreeW, &[readonly, 0], 300),
        Err(RegistryError32::AccessDenied)
    );
    assert_eq!(
        engine.registry.as_ref().unwrap().native.revision(),
        revision
    );
    assert_eq!(
        engine
            .registry
            .as_ref()
            .unwrap()
            .native
            .query_subkey_value(handle, &"New".encode_utf16().collect::<Vec<_>>(), 0, &[])
            .unwrap()
            .bytes,
        [7]
    );
}
#[test]
fn legacy_setvalue_requires_nonnull_data_and_ignores_cbdata() {
    let mut engine = engine();
    let handle = create(&mut engine);
    let revision = engine.registry.as_ref().unwrap().native.revision();
    assert_eq!(
        engine.registry_call(Api::RegSetValueW, &[handle, 0, 1, 0, u32::MAX], 200),
        Err(RegistryError32::InvalidParameter)
    );
    assert_eq!(
        engine.registry.as_ref().unwrap().native.revision(),
        revision
    );
    put(&mut engine, 0x2300, b"ok\0");
    assert_eq!(
        engine.registry_call(Api::RegSetValueA, &[handle, 0, 1, 0x2300, u32::MAX], 200),
        Ok(0)
    );
    assert_eq!(
        engine
            .registry
            .as_ref()
            .unwrap()
            .native
            .query_value(handle, &[])
            .unwrap()
            .bytes,
        [b'o', 0, b'k', 0, 0, 0]
    );
}
