use super::*;
use crate::{loader::ResourceIdentifier32, windows::user32::images::ImageKind32};

#[test]
fn static_resources_are_private_real_images_and_owned_retirement_releases_their_registry_slots() {
    let mut engine = fixture(false);
    for _ in 0..24 {
        for (kind, id) in [(ImageKind32::Bitmap, 7), (ImageKind32::Icon, 6)] {
            let handle = engine
                .load_static_control_image(BASE, ResourceIdentifier32::Id(id), kind, 0x800)
                .unwrap();
            assert!(
                !engine
                    .user32
                    .as_ref()
                    .unwrap()
                    .images
                    .record(handle, kind)
                    .unwrap()
                    .shared
            );
            if kind == ImageKind32::Bitmap {
                let info = engine
                    .gdi
                    .as_ref()
                    .unwrap()
                    .bitmap_info(engine.memory().unwrap(), handle)
                    .unwrap();
                assert_eq!((info.width, info.height, info.dib_section), (2, 2, false));
            } else {
                let info = engine.gdi.as_ref().unwrap().image_info(handle).unwrap();
                assert_eq!((info.width, info.height), (2, 2));
            }
            engine.release_static_control_image(handle, kind).unwrap();
            assert!(engine.check_user32_image(handle, kind).is_err());
            assert!(
                engine
                    .user32
                    .as_ref()
                    .unwrap()
                    .images
                    .record(handle, kind)
                    .is_err()
            );
        }
    }
    let icon = engine
        .load_static_control_image(BASE, ResourceIdentifier32::Id(6), ImageKind32::Icon, 0)
        .unwrap();
    let info = engine.gdi.as_ref().unwrap().image_info(icon).unwrap();
    assert_eq!((info.width, info.height), (32, 32));
    engine
        .release_static_control_image(icon, ImageKind32::Icon)
        .unwrap();
}

#[test]
fn static_image_load_failure_and_wrong_kind_release_preserve_live_images() {
    let mut engine = fixture(false);
    let bitmap = engine
        .load_static_control_image(BASE, ResourceIdentifier32::Id(7), ImageKind32::Bitmap, 0)
        .unwrap();
    assert!(
        engine
            .load_static_control_image(BASE, ResourceIdentifier32::Id(900), ImageKind32::Bitmap, 0)
            .is_err()
    );
    assert!(
        engine
            .release_static_control_image(bitmap, ImageKind32::Icon)
            .is_err()
    );
    assert!(
        engine
            .check_user32_image(bitmap, ImageKind32::Bitmap)
            .is_ok()
    );
    engine
        .release_static_control_image(bitmap, ImageKind32::Bitmap)
        .unwrap();
}

#[test]
fn static_owned_cleanup_accepts_an_image_already_deleted_by_guest() {
    let mut engine = fixture(false);
    let bitmap = engine
        .load_static_control_image(BASE, ResourceIdentifier32::Id(7), ImageKind32::Bitmap, 0)
        .unwrap();
    assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[bitmap]), 1);
    engine
        .release_static_control_image(bitmap, ImageKind32::Bitmap)
        .unwrap();
    engine
        .release_static_control_image(bitmap, ImageKind32::Bitmap)
        .unwrap();
    let icon = engine
        .load_static_control_image(BASE, ResourceIdentifier32::Id(6), ImageKind32::Icon, 0x800)
        .unwrap();
    assert_eq!(call(&mut engine, Api::DestroyIcon, &[icon]), 1);
    engine
        .release_static_control_image(icon, ImageKind32::Icon)
        .unwrap();
}

#[test]
fn forced_owner_cleanup_retains_foreign_dc_selection_then_reclaims_at_the_next_gdi_boundary() {
    use crate::windows::controls::ControlImage32;
    let mut engine = fixture(false);
    let bitmap = engine
        .load_static_control_image(BASE, ResourceIdentifier32::Id(7), ImageKind32::Bitmap, 0)
        .unwrap();
    let dc = gdi(&mut engine, GdiApi32::CreateCompatibleDC, &[0]);
    let old = gdi(&mut engine, GdiApi32::SelectObject, &[dc, bitmap]);
    let image = ControlImage32 {
        kind: ImageKind32::Bitmap,
        handle: bitmap,
    };
    engine
        .prepare_static_control_thread_retirement(&[image], 2)
        .unwrap();
    engine.gdi.as_mut().unwrap().retire_thread(2);
    engine.commit_static_control_thread_retirement(vec![image]);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .images
            .pending_retirement_count(),
        1
    );
    assert_ne!(gdi(&mut engine, GdiApi32::GetObjectType, &[bitmap]), 0);
    assert_eq!(gdi(&mut engine, GdiApi32::SelectObject, &[dc, old]), bitmap);
    assert_eq!(gdi(&mut engine, GdiApi32::GetObjectType, &[bitmap]), 0);
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .images
            .pending_retirement_count(),
        0
    );
    assert!(
        engine
            .check_user32_image(bitmap, ImageKind32::Bitmap)
            .is_err()
    );
}
