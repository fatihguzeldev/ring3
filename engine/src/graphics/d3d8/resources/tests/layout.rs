use super::*;

#[test]
fn compressed_cube_and_volume_mips_have_exact_pitch_and_independent_backing() {
    let mut store = store();
    let cube = store
        .create(texture(TextureKind::Cube, Format::Dxt1, Pool::Managed))
        .unwrap();
    assert_eq!(store.subresource_count(cube).unwrap(), 24);
    assert_eq!(
        store
            .info(SurfaceRef {
                resource: cube,
                subresource: 0
            })
            .unwrap(),
        SubresourceInfo {
            width: 8,
            height: 8,
            depth: 1,
            row_pitch: 16,
            slice_pitch: 32,
            length: 32,
            face: 0,
            level: 0
        }
    );
    let tail = store
        .info(SurfaceRef {
            resource: cube,
            subresource: 23,
        })
        .unwrap();
    assert_eq!(
        (tail.width, tail.height, tail.length, tail.face, tail.level),
        (1, 1, 8, 5, 3)
    );
    let volume = store
        .create(texture(TextureKind::Volume, Format::A8, Pool::SystemMemory))
        .unwrap();
    let info = store
        .info(SurfaceRef {
            resource: volume,
            subresource: 0,
        })
        .unwrap();
    assert_eq!(
        (info.row_pitch, info.slice_pitch, info.length),
        (8, 64, 256)
    );
    assert_eq!(
        store.resource_bytes(),
        6 * (32 + 8 + 8 + 8) + 256 + 32 + 4 + 1
    );
}

#[test]
fn failed_creation_and_generation_reuse_do_not_alias_or_consume_budget() {
    let mut store = store();
    let bad = ResourceDesc::Texture(TextureDesc {
        width: u32::MAX,
        height: u32::MAX,
        ..match texture(TextureKind::Texture2D, Format::A8, Pool::Managed) {
            ResourceDesc::Texture(desc) => desc,
            _ => unreachable!(),
        }
    });
    assert!(store.create(bad).is_err());
    assert_eq!(store.resource_bytes(), 0);
    let old = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    store.destroy(old).unwrap();
    let new = store
        .create(texture(TextureKind::Texture2D, Format::A8, Pool::Managed))
        .unwrap();
    assert_eq!(old.slot, new.slot);
    assert_ne!(old.generation, new.generation);
    assert_eq!(store.desc(old), Err(ResourceError::StaleResource));
}
