use super::*;

#[test]
fn gpu_writes_require_current_readback_and_default_resources_gate_reset() {
    let mut store = store();
    let id = store
        .create(ResourceDesc::Surface(SurfaceDesc {
            width: 2,
            height: 2,
            format: Format::A8R8G8B8,
            usage: Usage(Usage::RENDER_TARGET),
            pool: Pool::Default,
            multisample: 0,
            lockable: true,
        }))
        .unwrap();
    let surface = SurfaceRef {
        resource: id,
        subresource: 0,
    };
    store.mark_gpu_written(surface).unwrap();
    assert_eq!(store.bytes(surface), Err(ResourceError::ReadbackRequired));
    assert_eq!(
        store.lock_box(surface, None, LockFlags(LockFlags::READ_ONLY)),
        Err(ResourceError::ReadbackRequired)
    );
    assert_eq!(store.can_reset_default(), Err(ResourceError::InvalidCall));
    store.can_reset_default_except(&[id]).unwrap();
    let version = store.version(id).unwrap();
    assert_eq!(
        store.complete_readback(surface, version - 1, &[0; 16]),
        Err(ResourceError::StaleResource)
    );
    store.complete_readback(surface, version, &[3; 16]).unwrap();
    assert_eq!(store.bytes(surface).unwrap(), &[3; 16]);
    let lock = store
        .lock_box(surface, None, LockFlags(LockFlags::READ_ONLY))
        .unwrap();
    assert_eq!(
        store.can_reset_default_except(&[id]),
        Err(ResourceError::InvalidCall)
    );
    store.unlock(id, lock.token, None).unwrap();
    store.destroy(id).unwrap();
    store.can_reset_default().unwrap();
}

#[test]
fn private_data_replacement_and_removal_return_budget_without_changing_render_version() {
    let mut store = store();
    let id = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    let initial = store.resource_bytes();
    assert_eq!(
        store.private_data(id, [7; 16]),
        Err(ResourceError::NotFound)
    );
    store.set_private_data(id, [7; 16], &[1, 2, 3]).unwrap();
    assert_eq!(store.resource_bytes(), initial + 16 + 3);
    store.set_private_data(id, [7; 16], &[4]).unwrap();
    assert_eq!(store.private_data(id, [7; 16]).unwrap(), &[4]);
    assert_eq!(store.resource_bytes(), initial + 16 + 1);
    assert_eq!(store.version(id).unwrap(), 1);
    store.free_private_data(id, [7; 16]).unwrap();
    assert_eq!(store.resource_bytes(), initial);
    assert_eq!(
        store.free_private_data(id, [7; 16]),
        Err(ResourceError::NotFound)
    );
}
