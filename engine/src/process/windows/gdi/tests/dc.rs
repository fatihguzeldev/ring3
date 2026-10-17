use super::paint::{capture, eax, fixture_with_startup};
use crate::{memory::GuestAddress, windows::gdi::GdiApi32 as Api};

fn invoke(engine: &mut crate::process::EngineInstance, api: Api, args: &[u32]) -> u32 {
    capture(engine, api.id(), args);
    eax(engine)
}

#[test]
fn getdcex_transfers_real_region_ownership_and_system_clip_is_separate_from_app_clip() {
    let (mut engine, _, window) = fixture_with_startup(false);
    let region = invoke(&mut engine, Api::CreateRectRgn, &[1, 1, 5, 4]);
    let dc = invoke(&mut engine, Api::GetDCEx, &[window, region, 2 | 0x80]);
    assert_ne!(dc, 0);
    assert_eq!(invoke(&mut engine, Api::GetObjectType, &[region]), 0);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 1, 1]), 1);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 0, 0]), 0);
    let app = invoke(&mut engine, Api::CreateRectRgn, &[0, 0, 8, 6]);
    assert_eq!(invoke(&mut engine, Api::GetClipRgn, &[dc, app]), 0);
    assert_eq!(invoke(&mut engine, Api::GetClipBox, &[dc, 0x12c00]), 2);
    let mut rectangle = [0; 16];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x12c00), &mut rectangle)
        .unwrap();
    assert_eq!(
        rectangle.as_slice(),
        [1u32, 1, 5, 4]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>()
            .as_slice()
    );
    invoke(&mut engine, Api::SelectClipRgn, &[dc, app]);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 0, 0]), 0);
    assert_eq!(
        invoke(&mut engine, Api::SetPixel, &[dc, 1, 1, 0x123456]),
        0x123456
    );
    assert_eq!(invoke(&mut engine, Api::ReleaseDC, &[window, dc]), 1);
    assert_eq!(invoke(&mut engine, Api::DeleteObject, &[app]), 1);
}

#[test]
fn getdcex_invalid_flags_keep_region_live_and_failed_acquisition_neutral() {
    let (mut engine, _, window) = fixture_with_startup(false);
    let region = invoke(&mut engine, Api::CreateRectRgn, &[1, 1, 3, 3]);
    let count = engine.gdi.as_ref().unwrap().model.object_count();
    let bytes = engine.gdi.as_ref().unwrap().bytes;
    for flags in [0xc0, 0x10, 0x20, 0x80000000] {
        assert_eq!(
            invoke(&mut engine, Api::GetDCEx, &[window, region, flags]),
            0
        );
        assert!(engine.gdi.as_ref().unwrap().model.object(region).is_ok());
        assert_eq!(engine.gdi.as_ref().unwrap().model.object_count(), count);
        assert_eq!(engine.gdi.as_ref().unwrap().bytes, bytes);
    }
    assert_eq!(
        invoke(&mut engine, Api::GetDCEx, &[u32::MAX, region, 0x80]),
        0
    );
    assert!(engine.gdi.as_ref().unwrap().model.object(region).is_ok());
    let dc = invoke(&mut engine, Api::GetDCEx, &[window, region, 2 | 0x40]);
    assert_ne!(dc, 0);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 1, 1]), 0);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 0, 0]), 1);
    assert_eq!(invoke(&mut engine, Api::ReleaseDC, &[window, dc]), 1);
}

#[test]
fn getdcex_cache_bypasses_private_dc_and_keeps_exact_release_thread() {
    use crate::windows::gdi::{GdiError32, GdiWindow32};
    let (mut runtime, _) = super::fixture();
    let window = GdiWindow32 {
        window: 19,
        thread: 1,
        class_atom: 0xc001,
        class_style: 0x20,
        background_brush: 0,
        width: 8,
        height: 6,
        whole: false,
    };
    let private = runtime.acquire_dc(window, 1).unwrap();
    assert_eq!(runtime.acquire_dc(window, 1), Ok(private));
    let cached = runtime.acquire_dc_options(window, 2, true).unwrap();
    assert_ne!(private, cached);
    assert_eq!(runtime.release_window_dc(19, cached, 1), Ok(false));
    assert_eq!(runtime.release_window_dc(19, cached, 2), Ok(true));
    assert_eq!(
        runtime.release_window_dc(19, cached, 2),
        Err(GdiError32::InvalidHandle)
    );
    assert_eq!(runtime.acquire_dc(window, 1), Ok(private));
}

#[test]
fn getdcex_clipchildren_excludes_only_real_visible_child_rectangles() {
    let (mut engine, callback, window) = fixture_with_startup(false);
    capture(
        &mut engine,
        crate::windows::user32::User32Api32::CreateWindowExA.id(),
        &[
            0, 0x12100, 0, 0x50000000, 2, 1, 3, 2, window, 0, 0x400000, 0,
        ],
    );
    while let Some(request) = engine.pending_windows_callback() {
        super::paint::finish(
            &mut engine,
            callback,
            u32::from(request.arguments[1] == 0x81),
        );
    }
    assert_ne!(eax(&engine), 0);
    let dc = invoke(&mut engine, Api::GetDCEx, &[window, 0, 2 | 8]);
    assert_ne!(dc, 0);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 2, 1]), 0);
    assert_eq!(invoke(&mut engine, Api::PtVisible, &[dc, 0, 0]), 1);
    assert_eq!(
        invoke(&mut engine, Api::SetPixel, &[dc, 2, 1, 0x123456]),
        u32::MAX
    );
    assert_eq!(
        invoke(&mut engine, Api::SetPixel, &[dc, 0, 0, 0x123456]),
        0x123456
    );
    assert_eq!(invoke(&mut engine, Api::GetClipBox, &[dc, 0x12c00]), 3);
    assert_eq!(invoke(&mut engine, Api::ReleaseDC, &[window, dc]), 1);
}
