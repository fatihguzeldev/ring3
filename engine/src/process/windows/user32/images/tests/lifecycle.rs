use super::*;
use crate::windows::gdi::GdiError32;

#[test]
fn captured_images_share_only_requested_resources_and_preserve_copy_destroy_and_cursor_ownership() {
    let mut engine = fixture(false);
    let shared = call(&mut engine, Api::LoadImageW, &[BASE, 6, 1, 0, 0, 0x8000]);
    assert_ne!(shared, 0);
    assert_eq!(
        call(&mut engine, Api::LoadImageA, &[BASE, 6, 1, 16, 16, 0x8000]),
        shared
    );
    let info = engine.gdi.as_ref().unwrap().image_info(shared).unwrap();
    assert_eq!((info.width, info.height), (2, 2));
    assert_eq!(
        call(&mut engine, Api::CopyImage, &[shared, 1, 2, 2, 0xc]),
        shared
    );
    assert_eq!(call(&mut engine, Api::DestroyIcon, &[shared]), 1);
    assert!(engine.gdi.as_ref().unwrap().image_info(shared).is_ok());
    let icon = call(&mut engine, Api::LoadImageW, &[BASE, 6, 1, 0, 0, 0]);
    assert_ne!(icon, 0);
    assert_ne!(icon, shared);
    assert_eq!(call(&mut engine, Api::DestroyCursor, &[icon]), 0);
    assert!(engine.gdi.as_ref().unwrap().image_info(icon).is_ok());
    let cursor = call(&mut engine, Api::LoadImageW, &[BASE, 5, 2, 0, 0, 0]);
    assert_ne!(cursor, 0);
    let info = engine.gdi.as_ref().unwrap().image_info(cursor).unwrap();
    assert_eq!(
        (info.width, info.height, info.hotspot.x, info.hotspot.y),
        (2, 2, 1, 0)
    );
    assert_eq!(call(&mut engine, Api::SetCursor, &[cursor]), 0);
    let request = engine.pending_user32_cursor().unwrap();
    assert_eq!(request.cursor, cursor);
    assert_eq!(call(&mut engine, Api::DestroyCursor, &[cursor]), 0);
    assert!(engine.gdi.as_ref().unwrap().image_info(cursor).is_ok());
    assert_eq!(call(&mut engine, Api::SetCursor, &[0]), cursor);
    assert_eq!(call(&mut engine, Api::DestroyCursor, &[cursor]), 1);
    assert!(matches!(
        engine.gdi.as_ref().unwrap().image_info(cursor),
        Err(GdiError32::InvalidHandle)
    ));
    assert_eq!(call(&mut engine, Api::DestroyIcon, &[icon]), 1);
    assert!(matches!(
        engine.gdi.as_ref().unwrap().image_info(icon),
        Err(GdiError32::InvalidHandle)
    ));
}

#[test]
fn captured_bitmaps_use_canonical_ddb_dib_backing_and_reclaim_metadata_after_deleteobject() {
    let mut engine = fixture(false);
    let first = call(&mut engine, Api::LoadBitmapW, &[BASE, 7]);
    let second = call(&mut engine, Api::LoadBitmapW, &[BASE, 7]);
    assert_ne!(first, 0);
    assert_ne!(first, second);
    assert!(
        !engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), first)
            .unwrap()
            .dib_section
    );
    for handle in [first, second] {
        assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[handle]), 1);
    }
    let mapped = engine.memory.as_ref().unwrap().mapped_pages();
    for _ in 0..40 {
        let bitmap = call(&mut engine, Api::LoadBitmapA, &[BASE, 7]);
        assert_ne!(bitmap, 0);
        assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[bitmap]), 1);
    }
    assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), mapped);
    let bitmap = call(&mut engine, Api::LoadImageW, &[BASE, 7, 0, 0, 0, 0x2000]);
    assert_ne!(bitmap, 0);
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), bitmap)
            .unwrap()
            .dib_section
    );
    assert_eq!(
        gdi(&mut engine, GdiApi32::GetObjectW, &[bitmap, 24, DATA]),
        24
    );
    let pointer = read_word(&engine, DATA + 20);
    assert_ne!(pointer, 0);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(pointer), &[0x33, 0x22, 0x11, 0])
        .unwrap();
    let dc = gdi(&mut engine, GdiApi32::CreateCompatibleDC, &[0]);
    let old = gdi(&mut engine, GdiApi32::SelectObject, &[dc, bitmap]);
    assert_eq!(
        gdi(&mut engine, GdiApi32::GetPixel, &[dc, 0, 0]),
        0x00332211
    );
    assert_eq!(gdi(&mut engine, GdiApi32::SelectObject, &[dc, old]), bitmap);
    assert_eq!(gdi(&mut engine, GdiApi32::DeleteObject, &[bitmap]), 1);
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(pointer), &mut [0; 1])
            .is_err()
    );
    assert_eq!(gdi(&mut engine, GdiApi32::DeleteDC, &[dc]), 1);
}
