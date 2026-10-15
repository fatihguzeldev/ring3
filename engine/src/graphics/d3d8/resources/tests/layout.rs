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

#[test]
fn dxt_top_levels_are_block_aligned_but_tail_mips_and_yuv_width_are_not_conflated() {
    let mut store = store();
    for format in [
        Format::Dxt1,
        Format::Dxt2,
        Format::Dxt3,
        Format::Dxt4,
        Format::Dxt5,
    ] {
        let ResourceDesc::Texture(desc) = texture(TextureKind::Texture2D, format, Pool::Scratch)
        else {
            unreachable!()
        };
        assert_eq!(
            store.create(ResourceDesc::Texture(TextureDesc { width: 2, ..desc })),
            Err(ResourceError::InvalidCall)
        );
        let id = store.create(ResourceDesc::Texture(desc)).unwrap();
        assert_eq!(
            store
                .info(SurfaceRef {
                    resource: id,
                    subresource: 3
                })
                .unwrap()
                .width,
            1
        );
    }
    let ResourceDesc::Texture(desc) = texture(TextureKind::Texture2D, Format::Yuy2, Pool::Scratch)
    else {
        unreachable!()
    };
    let odd = store
        .create(ResourceDesc::Texture(TextureDesc {
            width: 3,
            levels: 1,
            ..desc
        }))
        .unwrap();
    assert_eq!(
        store
            .info(SurfaceRef {
                resource: odd,
                subresource: 0
            })
            .unwrap()
            .row_pitch,
        8
    );
}
