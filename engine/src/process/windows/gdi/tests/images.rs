use super::{call, fixture};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::gdi::{
        GdiDibSource32, GdiImageDraw32, GdiImageKind32, GdiImageRequest32, ResourceBitmapOptions32,
    },
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::Point32,
    },
};
fn dib_bytes(width: u32, height: i32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = [
        40,
        width,
        height as u32,
        1 | 32 << 16,
        0,
        pixels.len() as u32,
        0,
        0,
        0,
        0,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    bytes.extend_from_slice(pixels);
    bytes
}
fn icon_bytes() -> Vec<u8> {
    dib_bytes(
        2,
        4,
        &[
            0xff, 0, 0, 0, 0xff, 0xff, 0xff, 0, 0, 0, 0x80, 0x80, 0, 0x80, 0, 0xff, 0, 0, 0, 0, 0,
            0, 0, 0,
        ],
    )
}
fn request(bytes: &[u8]) -> GdiImageRequest32<'_> {
    GdiImageRequest32 {
        kind: GdiImageKind32::Icon,
        dib: GdiDibSource32 {
            info: bytes,
            pixels: None,
        },
        hotspot: Point32::default(),
        width: 0,
        height: 0,
        monochrome: false,
        transparent: false,
        map3d: false,
        vga: false,
    }
}
fn create(
    runtime: &mut super::GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    bytes: &[u8],
) -> u32 {
    let prepared = runtime
        .prepare_image_group(ram, request(bytes), None)
        .unwrap();
    runtime.commit_image_group(ram, prepared).unwrap()
}
#[test]
fn image_planes_use_canonical_opaque_identity_and_native_raw_alpha_composition() {
    let (mut runtime, mut ram) = fixture();
    let bytes = icon_bytes();
    let image = create(&mut runtime, &mut ram, &bytes);
    assert_eq!(
        runtime.model.object_type(image),
        Err(GdiError32::InvalidHandle)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[image]),
        Err(GdiError32::InvalidHandle)
    );
    let info = runtime.image_info(image).unwrap();
    assert_eq!(
        (info.width, info.height, info.hotspot, info.alpha),
        (2, 2, Point32 { x: 1, y: 1 }, true)
    );
    assert_eq!(&info.color.unwrap().pixels[..4], &[0, 0, 0x80, 0x80]);
    let pixels = dib_bytes(
        2,
        -2,
        &[255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
    );
    let prepared = runtime
        .prepare_resource_bitmap(
            &mut ram,
            GdiDibSource32 {
                info: &pixels,
                pixels: None,
            },
            ResourceBitmapOptions32 {
                dib_section: true,
                ..Default::default()
            },
            None,
        )
        .unwrap();
    let bitmap = runtime.commit_resource_bitmap(&mut ram, prepared).unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, image]),
        Err(GdiError32::InvalidHandle)
    );
    let index = runtime.dc_surface(dc).unwrap();
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    canvas
        .image(
            &runtime,
            GdiImageDraw32 {
                handle: image,
                position: Point32::default(),
                width: 2,
                height: 2,
                flags: 3,
                background: 0,
            },
        )
        .unwrap();
    runtime.publish_canvas(&mut ram, canvas).unwrap();
    let actual = runtime.surfaces[index].storage.read(&ram, 16).unwrap();
    assert_eq!(
        u32::from_le_bytes(actual[..4].try_into().unwrap()),
        0x8040007f,
        "native straight source alpha is preserved, not premultiplied at load"
    );
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    canvas
        .image(
            &runtime,
            GdiImageDraw32 {
                handle: image,
                position: Point32::default(),
                width: 2,
                height: 2,
                flags: 2,
                background: 0,
            },
        )
        .unwrap();
    runtime.publish_canvas(&mut ram, canvas).unwrap();
    let actual = runtime.surfaces[index].storage.read(&ram, 16).unwrap();
    assert_eq!(
        u32::from_le_bytes(actual[..4].try_into().unwrap()),
        0x80800000
    );
}
#[test]
fn image_replacement_reclaims_planes_with_fixed_live_budget_and_stale_receipts_fail() {
    let (_, mut ram) = fixture();
    let mut runtime = super::GdiRuntime32::new(super::GdiConfig32 {
        objects: 4,
        dcs: 2,
        saved_states: 2,
        bitmap_bytes: 20,
        bitmap_base: 0x20000,
        bitmap_pages: 4,
        screen_width: 1,
        screen_height: 1,
    })
    .unwrap();
    let bytes = icon_bytes();
    let mut image = create(&mut runtime, &mut ram, &bytes);
    let mapped = ram.mapped_pages();
    let version = ram.transaction_version();
    for _ in 0..40 {
        let prepared = runtime
            .prepare_copy_image(&mut ram, image, Default::default(), Some(image))
            .unwrap();
        let next = runtime.commit_image_group(&mut ram, prepared).unwrap();
        assert!(next > image);
        assert!(runtime.image_info(image).is_err());
        image = next;
        assert_eq!(runtime.bytes, 20);
        assert_eq!(runtime.model.object_count(), 1);
        assert_eq!(runtime.surfaces.len(), 2);
    }
    let delete = runtime.prepare_delete_image(&ram, image).unwrap();
    let prepared = runtime
        .prepare_copy_image(&mut ram, image, Default::default(), Some(image))
        .unwrap();
    let next = runtime.commit_image_group(&mut ram, prepared).unwrap();
    assert_eq!(
        runtime.check_delete_image(&ram, &delete),
        Err(GdiError32::InvalidHandle)
    );
    assert!(runtime.image_info(next).is_ok());
    let old = runtime.next_surface;
    assert!(matches!(
        runtime.prepare_copy_image(&mut ram, next, Default::default(), None),
        Err(GdiError32::Capacity)
    ));
    assert_eq!(runtime.next_surface, old);
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(ram.mapped_pages(), mapped);
    runtime.check_process_exit(&ram).unwrap();
    assert_eq!(runtime.process_exit_ranges().count(), 0);
    runtime.retire_process();
    assert_eq!(
        (
            runtime.bytes,
            runtime.surfaces.len(),
            runtime.model.object_count()
        ),
        (0, 0, 0)
    );
}
#[test]
fn bitmap_resource_pixel_gap_and_repeated_dib_replacement_preserve_mapping_ownership() {
    let (mut runtime, mut ram) = fixture();
    let mut info = dib_bytes(2, -1, &[]);
    info.extend_from_slice(b"gap not pixel data");
    let pixels = [0x11, 0x22, 0x33, 0, 0x44, 0x55, 0x66, 0];
    let options = ResourceBitmapOptions32 {
        dib_section: true,
        ..Default::default()
    };
    let prepared = runtime
        .prepare_resource_bitmap(
            &mut ram,
            GdiDibSource32 {
                info: &info,
                pixels: Some(&pixels),
            },
            options,
            None,
        )
        .unwrap();
    let mut bitmap = runtime.commit_resource_bitmap(&mut ram, prepared).unwrap();
    for _ in 0..40 {
        let prepared = runtime
            .prepare_copy_bitmap(&mut ram, bitmap, options, Some(bitmap))
            .unwrap();
        bitmap = runtime.commit_resource_bitmap(&mut ram, prepared).unwrap();
        assert_eq!(
            (
                runtime.bytes,
                runtime.model.object_count(),
                runtime.surfaces.len()
            ),
            (8, 1, 1)
        );
        assert_eq!(ram.mapped_pages(), 3);
        let index = runtime
            .surface_index(super::super::storage::SurfaceKey32::Bitmap(bitmap))
            .unwrap();
        assert_eq!(
            runtime.surfaces[index].storage.read(&ram, 8).unwrap(),
            pixels
        );
    }
    let index = runtime
        .surface_index(super::super::storage::SurfaceKey32::Bitmap(bitmap))
        .unwrap();
    let pointer = runtime.surfaces[index].storage.address().unwrap();
    let prepared = runtime
        .prepare_copy_bitmap(&mut ram, bitmap, options, Some(bitmap))
        .unwrap();
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    ram.unmap(range).unwrap();
    ram.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    ram.write(GuestAddress(pointer), b"new owner").unwrap();
    let version = ram.transaction_version();
    assert_eq!(
        runtime.commit_resource_bitmap(&mut ram, prepared),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(ram.transaction_version(), version);
    let mut check = [0; 9];
    ram.read(GuestAddress(pointer), &mut check).unwrap();
    assert_eq!(&check, b"new owner");
    assert_eq!(runtime.model.object_count(), 1);
}
#[test]
fn image_canvas_rejects_retired_source_without_publishing_old_pixels() {
    let (mut runtime, mut ram) = fixture();
    let image = create(&mut runtime, &mut ram, &icon_bytes());
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
    canvas
        .image(
            &runtime,
            GdiImageDraw32 {
                handle: image,
                position: Point32::default(),
                width: 1,
                height: 1,
                flags: 3,
                background: 0,
            },
        )
        .unwrap();
    let delete = runtime.prepare_delete_image(&ram, image).unwrap();
    runtime.commit_delete_image(&mut ram, delete).unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    let mapped = ram.mapped_pages();
    let before = runtime.surfaces[index]
        .storage
        .read(&ram, runtime.surfaces[index].layout.byte_len().unwrap())
        .unwrap();
    assert_eq!(
        runtime.publish_canvas(&mut ram, canvas),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(ram.mapped_pages(), mapped);
    assert_eq!(
        runtime.surfaces[index]
            .storage
            .read(&ram, before.len())
            .unwrap(),
        before
    );
    assert_eq!(runtime.surfaces[index].revision, revision);
}
