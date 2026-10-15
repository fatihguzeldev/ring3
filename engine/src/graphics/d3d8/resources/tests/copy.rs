use super::*;

#[test]
fn update_texture_matches_bottom_levels_and_consumes_source_dirty_state() {
    let mut store = store();
    let source = store
        .create(texture(
            TextureKind::Texture2D,
            Format::A8,
            Pool::SystemMemory,
        ))
        .unwrap();
    let destination = store
        .create(ResourceDesc::Texture(TextureDesc {
            width: 2,
            height: 2,
            levels: 2,
            ..match texture(TextureKind::Texture2D, Format::A8, Pool::Default) {
                ResourceDesc::Texture(desc) => desc,
                _ => unreachable!(),
            }
        }))
        .unwrap();
    let source_tail = SurfaceRef {
        resource: source,
        subresource: 2,
    };
    let lock = store.lock_box(source_tail, None, LockFlags(0)).unwrap();
    store
        .unlock(source, lock.token, Some(&[1, 2, 3, 4]))
        .unwrap();
    store.update_texture(source, destination).unwrap();
    assert_eq!(
        store
            .bytes(SurfaceRef {
                resource: destination,
                subresource: 0
            })
            .unwrap(),
        &[1, 2, 3, 4]
    );
    assert!(!store.is_dirty(source).unwrap());
    assert_eq!(store.version(destination).unwrap(), 2);
    store.update_texture(source, destination).unwrap();
    assert_eq!(store.version(destination).unwrap(), 2);
}

#[test]
fn copy_rects_prevalidates_every_rectangle_and_handles_translated_rows() {
    let mut store = store();
    let desc = ResourceDesc::Surface(SurfaceDesc {
        width: 4,
        height: 4,
        format: Format::A8,
        usage: Usage(0),
        pool: Pool::SystemMemory,
        multisample: 0,
        lockable: true,
    });
    let source = SurfaceRef {
        resource: store.create(desc).unwrap(),
        subresource: 0,
    };
    let destination = SurfaceRef {
        resource: store.create(desc).unwrap(),
        subresource: 0,
    };
    let lock = store.lock_box(source, None, LockFlags(0)).unwrap();
    store
        .unlock(
            source.resource,
            lock.token,
            Some(&(0..16).collect::<Vec<u8>>()),
        )
        .unwrap();
    let rect = Rect {
        left: 1,
        top: 1,
        right: 3,
        bottom: 3,
    };
    assert_eq!(
        store.copy_rects(source, destination, &[rect, rect], Some(&[(0, 0), (3, 3)])),
        Err(ResourceError::InvalidCall)
    );
    assert_eq!(store.version(destination.resource).unwrap(), 1);
    store
        .copy_rects(source, destination, &[rect], Some(&[(0, 0)]))
        .unwrap();
    assert_eq!(
        store.bytes(destination).unwrap(),
        &[5, 6, 0, 0, 9, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}
