use super::{call, fixture};
use crate::windows::{
    gdi::{GdiApi32, GdiWindow32},
    user32::Rect32,
};

#[test]
fn zero_extent_window_dc_has_empty_device_visibility_despite_one_pixel_backing() {
    for (width, height) in [(8, 0), (0, 8), (0, 0)] {
        let (mut runtime, mut ram) = fixture();
        let dc = runtime
            .acquire_dc(
                GdiWindow32 {
                    window: 7,
                    thread: 1,
                    class_atom: 3,
                    class_style: 0,
                    background_brush: 0,
                    width,
                    height,
                    whole: false,
                },
                1,
            )
            .unwrap();
        let before = runtime.surfaces[0].revision;
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetPixel, &[dc, 0, 0]),
            Ok(u32::MAX)
        );
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::SetPixel,
                &[dc, 0, 0, 0xffffff]
            ),
            Ok(u32::MAX)
        );
        assert_eq!(runtime.surfaces[0].revision, before);
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::PtVisible, &[dc, 0, 0]),
            Ok(0)
        );
        assert_eq!(
            call(&mut runtime, &mut ram, GdiApi32::GetClipBox, &[dc, 0x10000]),
            Ok(1)
        );
        assert_eq!(
            super::memory::rect(&ram, 0x10000).unwrap(),
            Rect32::default()
        );
        let mut canvas = runtime.begin_canvas(&ram, dc).unwrap();
        canvas.fill(
            &runtime,
            Rect32 {
                left: 0,
                top: 0,
                right: 8,
                bottom: 8,
            },
            0xffffff,
        );
        runtime.publish_canvas(&mut ram, canvas).unwrap();
        assert_eq!(runtime.surfaces[0].revision, before);
        assert!(runtime.release_window_dc(7, dc, 1).unwrap());
    }
}

#[test]
fn private_window_dc_resize_updates_device_visibility_without_changing_application_clip() {
    let (mut runtime, mut ram) = fixture();
    let mut window = GdiWindow32 {
        window: 7,
        thread: 1,
        class_atom: 3,
        class_style: 0x20,
        background_brush: 0,
        width: 8,
        height: 4,
        whole: false,
    };
    let dc = runtime.acquire_dc(window, 1).unwrap();
    let region = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateRectRgn,
        &[0, 0, 3, 3],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectClipRgn,
        &[dc, region],
    )
    .unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SaveDC, &[dc]).unwrap();
    let _ = runtime.release_window_dc(7, dc, 1).unwrap();
    window.height = 0;
    assert_eq!(runtime.acquire_dc(window, 1), Ok(dc));
    call(&mut runtime, &mut ram, GdiApi32::RestoreDC, &[dc, u32::MAX]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::GetPixel, &[dc, 0, 0]),
        Ok(u32::MAX)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::GetClipRgn, &[dc, region]),
        Ok(1)
    );
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetRgnBox,
            &[region, 0x10000]
        ),
        Ok(2)
    );
    assert_eq!(
        super::memory::rect(&ram, 0x10000).unwrap(),
        Rect32 {
            left: 0,
            top: 0,
            right: 3,
            bottom: 3
        }
    );
    call(&mut runtime, &mut ram, GdiApi32::SelectClipRgn, &[dc, 0]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::PtVisible, &[dc, 0, 0]),
        Ok(0)
    );
    let _ = runtime.release_window_dc(7, dc, 1).unwrap();
    window.height = 4;
    assert_eq!(runtime.acquire_dc(window, 1), Ok(dc));
    assert_eq!(
        call(&mut runtime, &mut ram, GdiApi32::PtVisible, &[dc, 0, 0]),
        Ok(1)
    );
}
