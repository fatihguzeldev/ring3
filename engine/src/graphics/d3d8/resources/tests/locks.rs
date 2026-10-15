use super::*;

#[test]
fn buffer_locks_are_balanced_and_invalid_unlock_keeps_data_and_version() {
    let mut store = store();
    let id = store
        .create(ResourceDesc::Buffer(BufferDesc {
            length: 16,
            kind: BufferKind::Vertex { fvf: 0 },
            usage: Usage(Usage::DYNAMIC),
            pool: Pool::Default,
        }))
        .unwrap();
    let first = store.lock_buffer(id, 4, 4, LockFlags(0)).unwrap();
    let second = store
        .lock_buffer(id, 12, 4, LockFlags(LockFlags::NO_OVERWRITE))
        .unwrap();
    assert!(store.is_locked(id).unwrap());
    assert_eq!(
        store.unlock(id, first.token, Some(&[1, 2])),
        Err(ResourceError::InvalidCall)
    );
    assert_eq!(store.version(id).unwrap(), 1);
    store.unlock(id, first.token, Some(&[1, 2, 3, 4])).unwrap();
    assert!(store.is_locked(id).unwrap());
    store.unlock(id, second.token, Some(&[5, 6, 7, 8])).unwrap();
    assert!(!store.is_locked(id).unwrap());
    assert_eq!(
        store
            .bytes(SurfaceRef {
                resource: id,
                subresource: 0
            })
            .unwrap(),
        &[0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8]
    );
    assert_eq!(store.version(id).unwrap(), 3);
}

#[test]
fn texture_locks_preserve_rows_outside_rectangle_and_readonly_is_neutral() {
    let mut store = store();
    let id = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    let surface = SurfaceRef {
        resource: id,
        subresource: 0,
    };
    let lock = store
        .lock_box(
            surface,
            Some(Box3 {
                left: 2,
                top: 2,
                front: 0,
                right: 4,
                bottom: 4,
                back: 1,
            }),
            LockFlags(0),
        )
        .unwrap();
    assert_eq!((lock.offset, lock.row_pitch, lock.length), (18, 8, 10));
    store
        .unlock(id, lock.token, Some(&[7, 8, 99, 99, 99, 99, 99, 99, 9, 10]))
        .unwrap();
    let bytes = store.bytes(surface).unwrap();
    assert_eq!(&bytes[18..28], &[7, 8, 0, 0, 0, 0, 0, 0, 9, 10]);
    let version = store.version(id).unwrap();
    let readonly = store
        .lock_box(surface, None, LockFlags(LockFlags::READ_ONLY))
        .unwrap();
    store.unlock(id, readonly.token, None).unwrap();
    assert_eq!(store.version(id).unwrap(), version);
}

#[test]
fn index_lock_ranges_are_bytes_and_discard_invalidates_entire_old_buffer() {
    let mut store = store();
    let id = store
        .create(ResourceDesc::Buffer(BufferDesc {
            length: 7,
            kind: BufferKind::Index {
                format: Format::Index16,
            },
            usage: Usage(Usage::DYNAMIC),
            pool: Pool::Default,
        }))
        .unwrap();
    let first = store.lock_buffer(id, 0, 0, LockFlags(0)).unwrap();
    store.unlock(id, first.token, Some(&[1; 7])).unwrap();
    let partial = store
        .lock_buffer(id, 1, 3, LockFlags(LockFlags::DISCARD))
        .unwrap();
    store.unlock(id, partial.token, Some(&[2, 3, 4])).unwrap();
    assert_eq!(
        store
            .bytes(SurfaceRef {
                resource: id,
                subresource: 0
            })
            .unwrap(),
        &[0, 2, 3, 4, 0, 0, 0]
    );
}

#[test]
fn invalid_dynamic_and_compressed_locks_leave_existing_data_unlocked() {
    let mut store = store();
    let id = store
        .create(texture(TextureKind::Texture2D, Format::Dxt1, Pool::Managed))
        .unwrap();
    let surface = SurfaceRef {
        resource: id,
        subresource: 0,
    };
    assert_eq!(
        store.lock_box(surface, None, LockFlags(LockFlags::DISCARD)),
        Err(ResourceError::InvalidCall)
    );
    assert_eq!(
        store.lock_box(
            surface,
            Some(Box3 {
                left: 1,
                top: 0,
                front: 0,
                right: 4,
                bottom: 4,
                back: 1
            }),
            LockFlags(0)
        ),
        Err(ResourceError::InvalidCall)
    );
    assert!(!store.is_locked(id).unwrap());
    assert_eq!(store.version(id).unwrap(), 1);
}
