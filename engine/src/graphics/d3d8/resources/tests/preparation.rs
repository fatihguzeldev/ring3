use super::*;

fn surface(width: u32) -> ResourceDesc {
    ResourceDesc::Surface(SurfaceDesc {
        width,
        height: 4,
        format: Format::A8,
        usage: Usage(0),
        pool: Pool::SystemMemory,
        multisample: 0,
        lockable: true,
    })
}

#[test]
fn replacement_reuses_released_budget_and_slots_while_preserving_other_resources() {
    let mut store = ResourceStore::new(
        17,
        GraphicsLimits {
            resources: 3,
            resource_bytes: 117,
            batch_bytes: 1024,
        },
    )
    .unwrap();
    let old = store.create(surface(4)).unwrap();
    let retained = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    let prepared = store
        .prepare_replace(&[old], &[surface(4), surface(4)])
        .unwrap();
    let ids = prepared.ids().to_vec();
    assert_eq!(store.resource_bytes(), 101);
    assert_eq!(ids[0].slot, old.slot);
    assert_ne!(ids[0].generation, old.generation);
    store.can_commit_replace(&prepared).unwrap();
    store.commit_replace(prepared).unwrap();
    assert_eq!(store.resource_bytes(), 117);
    assert_eq!(store.desc(old), Err(ResourceError::StaleResource));
    assert_eq!(store.version(retained).unwrap(), 1);
    for id in ids {
        assert_eq!(store.version(id).unwrap(), 1);
    }
}

#[test]
fn stale_prepared_create_and_copy_fail_before_mutation() {
    let mut store = store();
    let prepared = store.prepare_create(surface(4)).unwrap();
    let source = SurfaceRef {
        resource: store.create(surface(4)).unwrap(),
        subresource: 0,
    };
    assert_eq!(
        store.can_commit_create(&prepared),
        Err(ResourceError::StaleResource)
    );
    assert_eq!(
        store.commit_create(prepared),
        Err(ResourceError::StaleResource)
    );
    let destination = SurfaceRef {
        resource: store.create(surface(4)).unwrap(),
        subresource: 0,
    };
    let copy = store
        .prepare_copy_rects(source, destination, &[], None)
        .unwrap();
    let lock = store.lock_box(source, None, LockFlags(0)).unwrap();
    store
        .unlock(source.resource, lock.token, Some(&[3; 16]))
        .unwrap();
    assert_eq!(
        store.can_commit_copy(&copy),
        Err(ResourceError::StaleResource)
    );
    assert_eq!(store.commit_copy(copy), Err(ResourceError::StaleResource));
    assert_eq!(store.version(destination.resource).unwrap(), 1);
    assert_eq!(store.bytes(destination).unwrap(), &[0; 16]);
}
