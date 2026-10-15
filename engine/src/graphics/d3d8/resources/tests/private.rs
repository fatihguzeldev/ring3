use super::*;

#[test]
fn texture_and_level_view_private_namespaces_do_not_alias_and_cleanup_is_local() {
    let mut store = store();
    let id = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    let base = store.resource_bytes();
    let key = [7; 16];
    store.set_private_data(id, key, &[0]).unwrap();
    for (namespace, value) in [(11, 1), (22, 2), (33, 3)] {
        store
            .set_private_data_for(id, namespace, key, &[value])
            .unwrap();
    }
    assert_eq!(store.private_data(id, key), Ok(&[0][..]));
    for (namespace, value) in [(11, 1), (22, 2), (33, 3)] {
        assert_eq!(store.private_data_for(id, namespace, key), Ok(&[value][..]));
    }
    assert_eq!(store.free_private_namespace(id, 22), Ok(17));
    assert_eq!(
        store.private_data_for(id, 22, key),
        Err(ResourceError::NotFound)
    );
    assert_eq!(store.private_data_for(id, 11, key), Ok(&[1][..]));
    assert_eq!(store.private_data_for(id, 33, key), Ok(&[3][..]));
    assert_eq!(store.resource_bytes(), base + 3 * 17);
    assert_eq!(store.version(id), Ok(1));
    store.free_private_data_for(id, 33, key).unwrap();
    assert_eq!(store.free_private_namespace(id, 33), Ok(0));
    store.destroy(id).unwrap();
    assert_eq!(store.resource_bytes(), 0);
}

#[test]
fn prepared_private_data_is_budget_neutral_and_stale_same_size_replacement_is_rejected() {
    let mut store = ResourceStore::new(
        17,
        GraphicsLimits {
            resources: 1,
            resource_bytes: 128,
            batch_bytes: 1024,
        },
    )
    .unwrap();
    let id = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    let key = [7; 16];
    store.set_private_data_for(id, 11, key, &[1, 2, 3]).unwrap();
    let before = store.resource_bytes();
    assert!(matches!(
        store.prepare_private_data(id, 22, key, &[4; 24]),
        Err(ResourceError::ByteCapacity)
    ));
    assert_eq!(store.resource_bytes(), before);
    assert_eq!(
        store.private_data_for(id, 22, key),
        Err(ResourceError::NotFound)
    );
    let prepared = store.prepare_private_data(id, 11, key, &[4, 5, 6]).unwrap();
    store.can_commit_private_data(&prepared).unwrap();
    assert_eq!(store.private_data_for(id, 11, key), Ok(&[1, 2, 3][..]));
    assert_eq!(store.resource_bytes(), before);
    store.set_private_data_for(id, 11, key, &[7, 8, 9]).unwrap();
    assert_eq!(
        store.can_commit_private_data(&prepared),
        Err(ResourceError::StaleResource)
    );
    assert_eq!(
        store.commit_private_data(prepared),
        Err(ResourceError::StaleResource)
    );
    assert_eq!(store.private_data_for(id, 11, key), Ok(&[7, 8, 9][..]));
    let prepared = store.prepare_private_data(id, 22, key, &[]).unwrap();
    store.commit_private_data(prepared).unwrap();
    assert_eq!(store.private_data_for(id, 22, key), Ok(&[][..]));
    assert_eq!(store.version(id), Ok(1));
}
